# llm2books/stanza_segmenter.py

import re
from typing import List, Dict, Any
from . import llm_prompts
from .helper import initialize_llm_client
from .llm_logger import LLMLogger

def count_real_words(text: str) -> int:
    """Helper to count 'real' words, ignoring punctuation."""
    return len(re.findall(r'[a-zA-Z]+', text))

class LLMSegmenter:
    """
    A hybrid segmenter that uses an LLM to find initial semantic boundaries
    and then applies a robust, rule-based algorithm to merge short segments
    into perfectly-sized final phrases.
    """
    def __init__(self, language_name: str, config: Dict[str, Any], llm_logger: LLMLogger):
        self.language_name = language_name
        self.llm_client = initialize_llm_client("claude")
        self.llm_logger = llm_logger

        stages_config = config.get("stages", {})
        segmenter_config = stages_config.get("Segmenter", {})
        models_config = config.get("models", {})
        
        primary_model_key = segmenter_config.get("primary_model", "haiku")
        self.model_name = models_config.get(primary_model_key, {}).get("name")
        if not self.model_name:
            raise ValueError(f"Could not find model name for key '{primary_model_key}' in config.toml")
        
        self.system_prompt_template = llm_prompts.get_system_prompt("segment_sentence_universal", {})
        self.min_segment_words = 5
        self.MERGE_FORWARD_PUNCT = {'.', '!', '?', ':', ';'}
        self.OPENING_PUNCT = {'“', '"', '‘', '(', '[', '{', '¡', '¿'}

    def _get_initial_segments_from_llm(self, sentence_text: str) -> List[str]:
        """Phase 1: Get initial boundaries from the LLM."""
        system_prompt = self.system_prompt_template.replace("{{TEXT}}", sentence_text)
        try:
            message = self.llm_client.messages.create(
                model=self.model_name,
                system=system_prompt,
                messages=[{"role": "user", "content": "Please segment the text."}],
                max_tokens=2048,
                temperature=0.0,
            )
            raw_response = message.content[0].text if message.content else ""
            llm_segments = [seg.strip() for seg in raw_response.splitlines() if seg.strip()]

            # Basic validation: check if word content matches
            original_words = "".join(re.findall(r'[a-zA-Z]+', sentence_text.lower()))
            llm_words = "".join(re.findall(r'[a-zA-Z]+', " ".join(llm_segments).lower()))

            if original_words != llm_words:
                print(f"WARNING: LLM changed content for '{sentence_text}'. Falling back to single segment.")
                self.llm_logger.log_validation_failure("LLMSegmenter", sentence_text, raw_response, "Word content mismatch")
                return [sentence_text]
            
            # Use the LLM boundaries to perfectly slice the original text
            final_segments = []
            text_to_slice = sentence_text
            for i, segment_chunk in enumerate(llm_segments):
                if i < len(llm_segments) - 1:
                    chunk_word_count = len(re.findall(r'\S+', segment_chunk))
                    
                    slice_end_point = 0
                    words_scanned = 0
                    # Scan through original text to find the slice point
                    for match in re.finditer(r'\S+', text_to_slice):
                        words_scanned += 1
                        if words_scanned >= chunk_word_count:
                            slice_end_point = match.end()
                            break
                    
                    # Capture all trailing whitespace
                    while slice_end_point < len(text_to_slice) and text_to_slice[slice_end_point].isspace():
                        slice_end_point += 1

                    final_segments.append(text_to_slice[:slice_end_point])
                    text_to_slice = text_to_slice[slice_end_point:]
                else:
                    final_segments.append(text_to_slice)
            
            return final_segments

        except Exception as e:
            print(f"ERROR: LLM Segmenter API call failed for '{sentence_text}'. Error: {e}. Falling back.")
            return [sentence_text]

    def _merge_short_segments(self, segments: List[str]) -> List[str]:
        """Phase 2: Apply deterministic, rule-based merging."""
        if not segments: return []

        word_counts = [count_real_words(s) for s in segments]

        while True:
            min_idx = -1
            for i, count in enumerate(word_counts):
                if 0 < count < self.min_segment_words: # Ignore already empty segments
                    min_idx = i
                    break
            
            if min_idx == -1: break

            can_merge_backward = min_idx > 0
            can_merge_forward = min_idx < len(segments) - 1
            
            merge_backward = False # Default to merging forward
            
            if not can_merge_backward and not can_merge_forward: break
            elif can_merge_backward and not can_merge_forward: merge_backward = True
            elif not can_merge_backward and can_merge_forward: merge_backward = False
            else: # Has two neighbors, apply priority rules
                left_neighbor = segments[min_idx - 1].strip()
                right_neighbor = segments[min_idx].strip() # The small segment itself
                right_neighbor_plus_one = segments[min_idx + 1].strip()

                # Rule 1: Punctuation Priority
                if left_neighbor and left_neighbor[-1] in self.MERGE_FORWARD_PUNCT:
                    merge_backward = False # Has strong punctuation, prefer forward merge
                elif right_neighbor_plus_one and right_neighbor_plus_one[0] in self.OPENING_PUNCT:
                    merge_backward = True # Avoid merging into a quote, prefer backward merge
                # Rule 2: Smallest Neighbor Fallback
                else:
                    if word_counts[min_idx - 1] <= word_counts[min_idx + 1]:
                        merge_backward = True
                    else:
                        merge_backward = False
            
            if merge_backward:
                segments[min_idx - 1] += segments.pop(min_idx)
                word_counts[min_idx - 1] += word_counts.pop(min_idx)
            else:
                segments[min_idx] += segments.pop(min_idx + 1)
                word_counts[min_idx] += word_counts.pop(min_idx + 1)
        
        return [s for s in segments if s.strip()]

    def segment_sentence(self, sentence_text: str) -> List[str]:
        if not sentence_text or not sentence_text.strip():
            return []
        
        initial_segments = self._get_initial_segments_from_llm(sentence_text)
        
        if len(initial_segments) <= 1:
            return initial_segments

        final_segments = self._merge_short_segments(initial_segments)
        return final_segments


# Drop-in replacements
class EnglishStanzaProcessor(LLMSegmenter):
    def __init__(self, config: Dict[str, Any], llm_logger: LLMLogger):
        super().__init__("English", config, llm_logger)

class SpanishStanzaProcessor(LLMSegmenter):
    def __init__(self, config: Dict[str, Any], llm_logger: LLMLogger):
        super().__init__("Spanish", config, llm_logger)