# llm2books/stanza_segmenter.py

import re
from typing import List, Tuple, Optional
import stanza
from abc import ABC
from enum import Enum, auto

def count_real_words(text: str) -> int:
    return len(re.findall(r'\b\w+\b', text))

class Cut(Enum):
    UNKNOWN = auto()
    VETO = auto()
    MINOR = auto()
    MAJOR = auto()
    PRIORITY = auto()

class PhraseNode:
    def __init__(self, phrase_type: str, children: List['PhraseNode'] = None):
        self.phrase_type = phrase_type
        self.children = children if children else []
        self.start_idx = -1
        self.end_idx = -1

class StanzaLanguageProcessor(ABC):
    def __init__(self, lang_code: str):
        print(f"Initializing Stanza pipeline for '{lang_code}'...")
        try:
            self.nlp = stanza.Pipeline(lang_code, processors='tokenize,pos,constituency', use_gpu=False, logging_level='WARN')
            print(f"Stanza pipeline for '{lang_code}' loaded.")
        except Exception as e:
            print(f"Failed to load Stanza pipeline for {lang_code}. Error: {e}")
            raise
        self.min_segment_words: int = 5
        self.PRIORITY_PUNCTUATION = {';', ':'}
        self.NO_CUT_AFTER_XPOS = set()
        self.VERB_GROUP_XPOS = set()
        self.NO_CUT_BEFORE_XPOS = set()
        self.MAJOR_CUT_BEFORE_PHRASE = set()
        self.OPENING_PUNCT = {'“', '"', '‘', '(', '[', '{'}
        self.CLOSING_PUNCT = {'”', '"', '’', ')', ']', '}'}
    
    def _build_phrase_tree_with_indices(self, tree, current_idx: int) -> Tuple[Optional[PhraseNode], int]:
        if tree.is_leaf(): return None, current_idx + 1
        child_nodes = []
        start_idx = current_idx
        for child in tree.children:
            child_node, current_idx = self._build_phrase_tree_with_indices(child, current_idx)
            if child_node: child_nodes.append(child_node)
        node = PhraseNode(tree.label, child_nodes)
        node.start_idx = start_idx
        node.end_idx = current_idx - 1
        return node, current_idx

    def _build_parent_map(self, root_node: PhraseNode) -> dict[int, PhraseNode]:
        parent_map = {}
        stack = [root_node]
        while stack:
            current_node = stack.pop()
            for child in current_node.children:
                if not child.children: 
                    for i in range(child.start_idx, child.end_idx + 1): parent_map[i] = current_node
                else: stack.append(child)
        return parent_map

    def _apply_vetoes(self, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut]):
        self._apply_short_quote_protection(words, cut_markers)
        for i in range(len(cut_markers)):
            if cut_markers[i] == Cut.PRIORITY: continue
            if words[i].xpos in self.NO_CUT_AFTER_XPOS: cut_markers[i] = Cut.VETO
            if (i + 1 < len(words)) and words[i+1].xpos in self.NO_CUT_BEFORE_XPOS: cut_markers[i] = Cut.VETO
            if (words[i].xpos in self.VERB_GROUP_XPOS and (i + 1 < len(words)) and words[i+1].xpos in self.VERB_GROUP_XPOS):
                cut_markers[i] = Cut.VETO
            if words[i].text in self.CLOSING_PUNCT or ((i + 1 < len(words)) and words[i+1].text in self.OPENING_PUNCT):
                cut_markers[i] = Cut.VETO
    
    def _apply_short_quote_protection(self, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut], max_internal_words: int = 8):
        i=0
        while i < len(words):
            word = words[i]
            if word.text in self.OPENING_PUNCT:
                start_quote_idx = i
                for j in range(i + 1, len(words)):
                    if words[j].text in self.CLOSING_PUNCT:
                        end_quote_idx = j
                        word_count = sum(1 for k in range(start_quote_idx + 1, end_quote_idx) if words[k].upos != 'PUNCT')
                        if 0 < word_count <= max_internal_words:
                            for k in range(start_quote_idx, end_quote_idx):
                                if k < len(cut_markers) and cut_markers[k] != Cut.PRIORITY:
                                    cut_markers[k] = Cut.VETO
                        i = end_quote_idx
                        break
                else: i += 1
            else: i += 1

    def _apply_constituent_change_cuts(self, parent_map: dict, cut_markers: List[Cut]):
        for i in range(len(cut_markers)):
            if cut_markers[i] == Cut.UNKNOWN:
                parent1, parent2 = parent_map.get(i), parent_map.get(i + 1)
                if parent1 and parent2 and (parent1 is not parent2):
                    cut_markers[i] = Cut.MINOR

    def _apply_comma_conjunction_cuts(self, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut]):
        if len(words) < 2: return
        for i in range(len(words) - 1):
            if words[i].text == ',' and words[i+1].upos == 'CCONJ':
                cut_idx = i + 1
                if cut_idx < len(cut_markers) and cut_markers[cut_idx] == Cut.UNKNOWN:
                    cut_markers[cut_idx] = Cut.MAJOR

    def _apply_cuts_by_phrase(self, node: PhraseNode, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut]):
        # This will be overridden by language-specific classes
        pass
    
    def _apply_sbar_cuts(self, node: PhraseNode, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut]):
        # This will be overridden by language-specific classes
        pass

    #
    def _apply_final_merging(self, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut]) -> List[str]:
        if not words: return []

        # 1. Initial aggressive segmentation.
        segments = []
        current_chunk_text = [words[0].text]
        for i, cut in enumerate(cut_markers):
            if cut != Cut.VETO:
                segments.append(" ".join(current_chunk_text))
                current_chunk_text = [words[i+1].text]
            else:
                current_chunk_text.append(words[i+1].text)
        segments.append(" ".join(current_chunk_text))
        
        # 2. Create a parallel list of boundary strengths.
        boundaries = []
        for cut in cut_markers:
            if cut == Cut.VETO: continue
            if cut == Cut.PRIORITY: boundaries.append(2)
            elif cut == Cut.MAJOR: boundaries.append(1)
            else: boundaries.append(0)

        # 3. Iteratively merge until all segments are valid.
        while True:
            word_counts = [count_real_words(s) for s in segments]
            
            min_idx = -1
            for i, count in enumerate(word_counts):
                if count < self.min_segment_words:
                    min_idx = i
                    break
            
            if min_idx == -1 or len(segments) == 1: break

            # --- THE FINAL, REFINED MERGE DECISION LOGIC ---
            merge_backward = False
            
            can_merge_backward = min_idx > 0
            can_merge_forward = min_idx < len(segments) - 1

            if not can_merge_backward and not can_merge_forward:
                # Should not happen if len(segments) > 1, but as a safeguard.
                break 
            elif can_merge_backward and not can_merge_forward:
                # Last segment is too small, MUST merge backward.
                merge_backward = True
            elif not can_merge_backward and can_merge_forward:
                # First segment is too small, MUST merge forward.
                merge_backward = False
            else: # Can merge in either direction. This is where the choice matters.
                boundary_before = boundaries[min_idx - 1]
                boundary_after = boundaries[min_idx]

                # Your core insight: Prefer merging with the smallest neighbor across the weakest boundary.
                if boundary_before < boundary_after:
                    merge_backward = True
                elif boundary_after < boundary_before:
                    merge_backward = False
                else: # Boundaries have equal strength, merge with the smaller neighbor.
                    if word_counts[min_idx - 1] <= word_counts[min_idx + 1]:
                        merge_backward = True
                    else:
                        merge_backward = False
            
            # Rebuild the lists after merging to avoid index errors.
            new_segments, new_boundaries = [], []
            if merge_backward:
                for i in range(len(segments)):
                    if i == min_idx - 1:
                        new_segments.append(f"{segments[i]} {segments[i+1]}")
                    elif i == min_idx: continue
                    else: new_segments.append(segments[i])
                for i in range(len(boundaries)):
                    if i != min_idx - 1: new_boundaries.append(boundaries[i])
            else: # Merge forward
                for i in range(len(segments)):
                    if i == min_idx:
                        new_segments.append(f"{segments[i]} {segments[i+1]}")
                    elif i == min_idx + 1: continue
                    else: new_segments.append(segments[i])
                for i in range(len(boundaries)):
                    if i != min_idx: new_boundaries.append(boundaries[i])
            
            segments = new_segments
            boundaries = new_boundaries

        # 4. Final cleanup of whitespace and punctuation.
        final_list = []
        for seg in segments:
            clean_seg = re.sub(r'\s+', ' ', seg).strip()
            clean_seg = re.sub(r'\s+([,.:;?!])', r'\1', clean_seg)
            clean_seg = re.sub(r'([‘“])\s+', r'\1', clean_seg)
            if clean_seg: final_list.append(clean_seg)

        return final_list

    def segment_sentence(self, sentence_text: str) -> List[str]:
        cleaned_sentence = re.sub(r'\s+', ' ', sentence_text).strip()
        if not cleaned_sentence: return []
        doc = self.nlp(cleaned_sentence)
        if not doc.sentences: return [cleaned_sentence]
        sent = doc.sentences[0]
        words = sent.words
        num_cuts = len(words) - 1
        if num_cuts < 0: return [cleaned_sentence]
        root_node, _ = self._build_phrase_tree_with_indices(sent.constituency, 0)
        if not root_node: return [cleaned_sentence]
        cut_markers: List[Cut] = [Cut.UNKNOWN] * num_cuts
        for i, word in enumerate(words[:-1]):
            if word.text in self.PRIORITY_PUNCTUATION: cut_markers[i] = Cut.PRIORITY
        
        self._apply_sbar_cuts(root_node, words, cut_markers)
        self._apply_comma_conjunction_cuts(words, cut_markers)
        self._apply_vetoes(words, cut_markers)
        self._apply_cuts_by_phrase(root_node, words, cut_markers)
        parent_map = self._build_parent_map(root_node)
        self._apply_constituent_change_cuts(parent_map, cut_markers)
        for i in range(num_cuts):
            if cut_markers[i] == Cut.UNKNOWN: cut_markers[i] = Cut.MINOR
        return self._apply_final_merging(words, cut_markers)

class EnglishStanzaProcessor(StanzaLanguageProcessor):
    def __init__(self):
        super().__init__('en')
        self.min_segment_words = 5
        self.NO_CUT_AFTER_XPOS = {'IN', 'DT', 'MD', 'TO', 'WDT', 'WP', 'WP$', 'POS'}
        self.VERB_GROUP_XPOS = {'MD', 'VB', 'VBD', 'VBG', 'VBN', 'VBP', 'VBZ', 'RB', 'RP'}
        self.NO_CUT_BEFORE_XPOS = {'POS', 'RP'}
        self.MAJOR_CUT_BEFORE_PHRASE = {'SBAR', 'PP'}

    def _apply_sbar_cuts(self, node: PhraseNode, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut]):
        if node.phrase_type == 'SBAR':
            comma_idx = node.end_idx + 1
            if comma_idx < len(words) and words[comma_idx].text == ',':
                cut_location = comma_idx
                if cut_location < len(cut_markers):
                    cut_markers[cut_location] = Cut.PRIORITY
        for child in node.children:
            self._apply_sbar_cuts(child, words, cut_markers)

    def _apply_cuts_by_phrase(self, node: PhraseNode, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut]):
        if node.phrase_type in self.MAJOR_CUT_BEFORE_PHRASE:
            word_count = node.end_idx - node.start_idx + 1
            if node.phrase_type == 'PP' and word_count <= 2: pass
            else:
                cut_idx = node.start_idx - 1
                if 0 <= cut_idx < len(cut_markers) and cut_markers[cut_idx] not in [Cut.VETO, Cut.PRIORITY]:
                    cut_markers[cut_idx] = Cut.MAJOR
        for child in node.children:
            self._apply_cuts_by_phrase(child, words, cut_markers)

class SpanishStanzaProcessor(StanzaLanguageProcessor):
    def __init__(self):
        super().__init__('es')
        self.min_segment_words = 5
        self.PRIORITY_PUNCTUATION = {';', ':'}
        self.NO_CUT_AFTER_XPOS = {'ADP', 'DET', 'SCONJ', 'CCONJ'}
        self.VERB_GROUP_XPOS = {'VERB', 'AUX'}
        self.OPENING_PUNCT = {'“', '"', '‘', '(', '[', '{', '¡', '¿'}
        self.CLOSING_PUNCT = {'”', '"', '’', ')', ']', '}', '!', '?'}
        self.MAJOR_CUT_BEFORE_PHRASE = {'sp', 'S', 'relatiu', 'conj'}

    def _apply_sbar_cuts(self, node: PhraseNode, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut]):
        # This rule handles subordinate clauses like "lo cual..." or "que..."
        if node.phrase_type in ['relatiu', 'conj'] or (node.phrase_type == 'S' and node.start_idx > 0):
            comma_idx = node.end_idx + 1
            if comma_idx < len(words) and words[comma_idx].text == ',':
                cut_location = comma_idx
                if cut_location < len(cut_markers) and cut_markers[cut_location] != Cut.VETO:
                    cut_markers[cut_location] = Cut.PRIORITY
        for child in node.children:
            self._apply_sbar_cuts(child, words, cut_markers)

    def _apply_cuts_by_phrase(self, node: PhraseNode, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut]):
        if node.phrase_type in self.MAJOR_CUT_BEFORE_PHRASE:
            word_count = node.end_idx - node.start_idx + 1
            if node.phrase_type == 'sp' and word_count <= 3:
                pass
            # Don't cut before a main clause 'S' unless it follows a comma or conjunction
            elif node.phrase_type == 'S':
                if node.start_idx > 0:
                    prev_word = words[node.start_idx - 1]
                    if prev_word.text == ',' or prev_word.upos == 'CCONJ':
                         cut_idx = node.start_idx - 1
                         if 0 <= cut_idx < len(cut_markers) and cut_markers[cut_idx] not in [Cut.VETO, Cut.PRIORITY]:
                            cut_markers[cut_idx] = Cut.MAJOR
            else:
                cut_idx = node.start_idx - 1
                if 0 <= cut_idx < len(cut_markers) and cut_markers[cut_idx] not in [Cut.VETO, Cut.PRIORITY]:
                    cut_markers[cut_idx] = Cut.MAJOR
        for child in node.children:
            self._apply_cuts_by_phrase(child, words, cut_markers)