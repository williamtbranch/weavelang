# Save as: stanza_segmenter.py
import re
from typing import List, Tuple, Optional
import stanza
from abc import ABC

# ... (PhraseNode class is unchanged) ...
class PhraseNode:
    def __init__(self, phrase_type: str, text: str, children: List['PhraseNode'] = None):
        self.phrase_type = phrase_type
        self.text = text
        self.children = children if children else []
        self.start_idx = -1
        self.end_idx = -1
    def __repr__(self):
        return f"({self.phrase_type}: '{self.text}')"

class StanzaLanguageProcessor(ABC):
    # ... (Class variables are unchanged) ...
    PRIORITY_PUNCTUATION = {';', ':'}
    NO_CUT_AFTER_XPOS = {'IN', 'DT', 'MD', 'TO', 'POS'}
    VERB_GROUP_XPOS = {'MD', 'VB', 'VBD', 'VBG', 'VBN', 'VBP', 'VBZ', 'RB', 'RP'}
    NO_CUT_BEFORE_XPOS = {'POS'}
    NO_CUT_BEFORE_PUNCT = {',', '.', ';', ':', '!', '?'} 
    MAJOR_CUT_BEFORE_PHRASE = {'PP', 'SBAR', 'LST'} 
    BINDS_RIGHT_XPOS = VERB_GROUP_XPOS.union({'NN', 'NNS', 'NNP', 'NNPS', 'PRP', 'PRP$', 'JJ', 'JJR', 'JJS', 'CC'})
    OPENING_PUNCT = {'“', '"', '‘', '(', '[', '{'}
    CLOSING_PUNCT = {'”', '"', '’', ')', ']', '}'}

    def __init__(self, lang_code: str):
        # ... (unchanged) ...
        print(f"Initializing Stanza pipeline for '{lang_code}'...")
        try:
            self.nlp = stanza.Pipeline(lang_code, processors='tokenize,pos,constituency', use_gpu=False, logging_level='WARN')
            print(f"Stanza pipeline for '{lang_code}' loaded.")
        except Exception as e:
            print(f"Failed to load Stanza pipeline for {lang_code}. Error: {e}")
            raise

    # ... (_get_clean_text, _build_phrase_tree_with_indices are unchanged) ...
    def _get_clean_text(self, tree) -> str:
        if tree.is_leaf(): return tree.label
        text_parts = [self._get_clean_text(child) for child in tree.children]
        return " ".join(text_parts)
    def _build_phrase_tree_with_indices(self, tree, current_idx: int) -> Tuple[Optional[PhraseNode], int]:
        if tree.is_leaf(): return None, current_idx + 1
        child_nodes = []
        start_idx = current_idx
        for child in tree.children:
            child_node, current_idx = self._build_phrase_tree_with_indices(child, current_idx)
            if child_node: child_nodes.append(child_node)
        node_text = self._get_clean_text(tree)
        node = PhraseNode(tree.label, node_text, child_nodes)
        node.start_idx = start_idx
        node.end_idx = current_idx - 1
        return node, current_idx

    # MODIFIED: Removed min_segment_len and the call to the validation function
    def get_sentence_with_cuts(self, sentence_text: str, min_phrase_len: int = 3) -> str:
        if not sentence_text.strip(): return ""
        doc = self.nlp(sentence_text)
        if not doc.sentences: return sentence_text
        sent = doc.sentences[0]
        words = sent.words
        root_node, _ = self._build_phrase_tree_with_indices(sent.constituency, 0)
        if not root_node: return sentence_text
        num_potential_cuts = len(words) - 1
        if num_potential_cuts < 0: return sentence_text

        cut_markers: List[Optional[str]] = ['?'] * num_potential_cuts

        self._apply_priority_punctuation_cuts(words, cut_markers)
        self._apply_sbar_cuts(root_node, words, cut_markers)
        self._apply_vetoes(words, root_node, cut_markers, min_phrase_len)
        self._apply_major_cuts(root_node, cut_markers)
        parent_map = self._build_parent_map(root_node)
        self._apply_minor_cuts(words, parent_map, cut_markers)

        result_parts = []
        for i, word in enumerate(words):
            result_parts.append(word.text)
            if i < num_potential_cuts and cut_markers[i] not in [None, '?']:
                result_parts.append(cut_markers[i])
        
        reconstructed = " ".join(result_parts)
        reconstructed = re.sub(r'\s*([|><#])\s*', r' \1 ', reconstructed)
        return reconstructed.strip()

    # ... (_apply_priority_punctuation_cuts, _apply_sbar_cuts are unchanged) ...
    def _apply_priority_punctuation_cuts(self, words: List[stanza.models.common.doc.Word], cut_markers: List[Optional[str]]):
        for i, word in enumerate(words):
            if word.text in self.PRIORITY_PUNCTUATION:
                if i < len(cut_markers): cut_markers[i] = '#'
    def _apply_sbar_cuts(self, node: PhraseNode, words: List[stanza.models.common.doc.Word], cut_markers: List[Optional[str]]):
        if node.phrase_type == 'SBAR':
            comma_idx = node.end_idx + 1
            if comma_idx < len(words) and words[comma_idx].text == ',':
                cut_location = comma_idx
                if cut_location < len(cut_markers): cut_markers[cut_location] = '#'
        for child in node.children: self._apply_sbar_cuts(child, words, cut_markers)
    
    # MODIFIED: Re-instated the short quote protection rule
    def _apply_vetoes(self, words, root_node, cut_markers, min_len):
        """Applies all rules that forbid a cut."""
        self._apply_short_quote_protection(words, cut_markers) # Re-added
        self._apply_small_phrase_veto(root_node, cut_markers, min_len)
        self._apply_punctuation_veto(words, cut_markers)
        self._apply_pos_vetoes(words, cut_markers)
        self._apply_hyphen_veto(words, cut_markers)

    # ... (_apply_major_cuts, _apply_minor_cuts are unchanged from two versions ago) ...
    def _apply_major_cuts(self, node: PhraseNode, cut_markers: List[Optional[str]]):
        should_cut = False
        if node.phrase_type in self.MAJOR_CUT_BEFORE_PHRASE:
            should_cut = True
            if node.phrase_type == 'PP':
                word_count = node.end_idx - node.start_idx + 1
                if word_count <= 2: should_cut = False
        if should_cut:
            cut_idx = node.start_idx - 1
            if 0 <= cut_idx < len(cut_markers) and cut_markers[cut_idx] not in ['#', None]:
                cut_markers[cut_idx] = '|'
        for child in node.children: self._apply_major_cuts(child, cut_markers)

    # NEW: Re-adding the short quote veto logic.
    def _apply_short_quote_protection(self, words: List[stanza.models.common.doc.Word], cut_markers: List[Optional[str]], max_internal_words: int = 8):
        """Finds short quoted phrases and vetoes any cuts within them. 
           An 8-word internal limit prevents any split that would result in a sub-5-word segment.
        """
        i = 0
        while i < len(words):
            word = words[i]
            if word.text in self.OPENING_PUNCT:
                start_quote_idx = i
                for j in range(i + 1, len(words)):
                    if words[j].text in self.CLOSING_PUNCT:
                        end_quote_idx = j
                        word_count = end_quote_idx - start_quote_idx - 1
                        if 0 < word_count <= max_internal_words:
                            for k in range(start_quote_idx, end_quote_idx):
                                if k < len(cut_markers) and cut_markers[k] != '#':
                                    cut_markers[k] = None
                        i = end_quote_idx
                        break
            i += 1
            
    # ... (Rest of the file is unchanged) ...
    def _apply_minor_cuts(self, words, parent_map, cut_markers):
        for i in range(len(cut_markers)):
            if cut_markers[i] == '?': 
                parent1 = parent_map.get(i)
                parent2 = parent_map.get(i + 1)
                if parent1 is not parent2: cut_markers[i] = '<' 
                elif words[i].xpos in self.BINDS_RIGHT_XPOS: cut_markers[i] = '>'
                else: cut_markers[i] = '<'
    def _apply_pos_vetoes(self, words: List[stanza.models.common.doc.Word], cut_markers: List[Optional[str]]):
        num_potential_cuts = len(words)
        for i in range(num_potential_cuts - 1):
            if cut_markers[i] == '#': continue
            if words[i].xpos in self.NO_CUT_AFTER_XPOS: cut_markers[i] = None
            if (words[i].xpos in self.VERB_GROUP_XPOS and words[i+1].xpos in self.VERB_GROUP_XPOS and words[i+1].xpos != 'IN'): cut_markers[i] = None
            if words[i+1].xpos in self.NO_CUT_BEFORE_XPOS: cut_markers[i] = None
            if words[i+1].text in self.NO_CUT_BEFORE_PUNCT: cut_markers[i] = None
    def _apply_punctuation_veto(self, words: List[stanza.models.common.doc.Word], cut_markers: List[Optional[str]]):
        for i, word in enumerate(words):
            if word.text in self.OPENING_PUNCT and i < len(cut_markers):
                if cut_markers[i] != '#': cut_markers[i] = None
            if word.text in self.CLOSING_PUNCT and i > 0:
                if cut_markers[i-1] != '#': cut_markers[i-1] = None
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
    def _apply_hyphen_veto(self, words: List[stanza.models.common.doc.Word], cut_markers: List[Optional[str]]):
        for i, word in enumerate(words):
            if word.text == '-':
                if i > 0 and cut_markers[i-1] != '#': cut_markers[i-1] = None
                if i < len(cut_markers) and cut_markers[i] != '#': cut_markers[i] = None
    def _apply_small_phrase_veto(self, node: PhraseNode, cut_markers: List[Optional[str]], min_len: int):
        word_count = node.end_idx - node.start_idx + 1
        if word_count <= min_len:
            for i in range(node.start_idx, node.end_idx):
                if 0 <= i < len(cut_markers):
                    if cut_markers[i] != '#': cut_markers[i] = None
            return
        for child in node.children: self._apply_small_phrase_veto(child, cut_markers, min_len)
    def get_atomic_phrase_tree_for_sentence(self, sentence_text: str) -> List[PhraseNode]:
        cleaned_sentence = re.sub(r'\s+', ' ', sentence_text).strip()
        if not cleaned_sentence: return []
        doc = self.nlp(cleaned_sentence)
        if not doc.sentences: return []
        tree = doc.sentences[0].constituency
        if not tree or not tree.children: return []
        def build_simple_tree(t):
            child_nodes = [build_simple_tree(child) for child in t.children if not child.is_leaf()]
            node_text = self._get_clean_text(t)
            return PhraseNode(t.label, node_text, child_nodes)
        return [build_simple_tree(child) for child in tree.children[0].children if not child.is_leaf()]

class EnglishStanzaProcessor(StanzaLanguageProcessor):
    def __init__(self): super().__init__('en')
class SpanishStanzaProcessor(StanzaLanguageProcessor):
    def __init__(self): super().__init__('es')