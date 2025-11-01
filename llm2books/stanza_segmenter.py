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
    and then applies a robust, rule-based algorithm to merge short segments.
    """
    def __init__(self, language_name: str, config: Dict[str, Any], llm_logger: LLMLogger):
        self.language_name = language_name
        self.llm_logger = llm_logger

        stages_config = config.get("stages", {})
        segmenter_config = stages_config.get("Segmenter", {})
        models_config = config.get("models", {})
        
        primary_model_key = segmenter_config.get("primary_model", "haiku")
        model_info = models_config.get(primary_model_key, {})
        
        self.model_name = model_info.get("name")
        self.provider = model_info.get("provider") # <-- Get the provider

        if not self.model_name or not self.provider:
            raise ValueError(f"Could not find model name or provider for key '{primary_model_key}' in config.toml")

        # --- THIS IS THE FIX ---
        # Initialize the client based on the configured provider
        self.llm_client = initialize_llm_client(self.provider)
        if not self.llm_client:
            raise ConnectionError(f"Failed to initialize LLM client for provider '{self.provider}'")
        # --- END OF FIX ---
        
        self.system_prompt_template = llm_prompts.get_system_prompt("segment_sentence_universal", {})
        self.min_segment_words = 5
        self.MERGE_FORWARD_PUNCT = {'.', '!', '?', ':', ';'}
        self.OPENING_PUNCT = {'“', '"', '‘', '(', '[', '{', '¡', '¿'}

    def _get_initial_segments_from_llm(self, sentence_text: str, s_id: str) -> List[str]:
        system_prompt = self.system_prompt_template.replace("{{TEXT}}", sentence_text)
        
        raw_response = ""
        try:
            # --- THIS IS THE FIX ---
            # Use the provider to determine which API call to make
            if self.provider == "claude":
                message = self.llm_client.messages.create(
                    model=self.model_name,
                    system=system_prompt,
                    messages=[{"role": "user", "content": "Please segment the text."}],
                    max_tokens=2048,
                    temperature=0.0,
                )
                raw_response = message.content[0].text if message.content else ""
            elif self.provider == "gemini":
                model = self.llm_client.GenerativeModel(self.model_name)
                # For Gemini, the system prompt is just part of the content list
                response = model.generate_content([system_prompt, "Please segment the text."])
                raw_response = response.text
            else:
                raise ValueError(f"Unsupported provider for segmentation: {self.provider}")
            # --- END OF FIX ---
        except Exception as e:
            raise IOError(f"LLM API call failed during segmentation for S_ID {s_id}: {e}")

        # ... (rest of the file is unchanged) ...
        if self.llm_logger:
            user_prompt_for_log = f"S_ID: {s_id}\n\nPlease segment the text."
            self.llm_logger.log_batch(
                job_name="LLMSegmenter",
                system_prompt=system_prompt, 
                user_prompt=user_prompt_for_log,
                response=raw_response
            )
        
        llm_segments = [seg.strip() for seg in raw_response.splitlines() if seg.strip()]
        original_words_norm = "".join(re.findall(r'[a-zA-Z0-9]+', sentence_text.lower()))
        llm_words_norm = "".join(re.findall(r'[a-zA-Z0-9]+', "".join(llm_segments).lower()))

        if original_words_norm != llm_words_norm:
            if self.llm_logger: self.llm_logger.log_validation_failure("LLMSegmenter", sentence_text, raw_response, "Word content mismatch")
            raise ValueError(f"LLM content mismatch for S_ID {s_id}. LLM modified word content.")
        
        final_segments = []
        current_search_offset = 0
        
        for i, segment_chunk in enumerate(llm_segments):
            if not segment_chunk.strip(): continue
            if i == len(llm_segments) - 1:
                remaining_text = sentence_text[current_search_offset:]
                if remaining_text: final_segments.append(remaining_text)
                break
            chunk_words = re.findall(r'[\w\']+', segment_chunk)
            if not chunk_words: continue
            anchor_word = chunk_words[-1]
            
            def normalize_quotes(text: str) -> str: return text.replace('’', "'").replace('‘', "'")
            normalized_anchor = normalize_quotes(anchor_word)
            search_text_slice = sentence_text[current_search_offset:]
            normalized_search_slice = normalize_quotes(search_text_slice)
            
            possible_matches = list(re.finditer(re.escape(normalized_anchor), normalized_search_slice))

            if not possible_matches:
                if self.llm_logger: self.llm_logger.log_validation_failure("LLMSegmenter", sentence_text, "\n".join(llm_segments), f"Anchor word '{anchor_word}' not found.")
                raise ValueError(f"Segmenter Integrity Check FAILED for S_ID {s_id}: Could not find anchor word '{anchor_word}'.")
            
            match = possible_matches[0]
            slice_end_point_relative = match.end()
            slice_end_point_absolute = current_search_offset + slice_end_point_relative
            while slice_end_point_absolute < len(sentence_text) and not sentence_text[slice_end_point_absolute].isalnum():
                if sentence_text[slice_end_point_absolute] in self.OPENING_PUNCT: break
                slice_end_point_absolute += 1
            while slice_end_point_absolute < len(sentence_text) and sentence_text[slice_end_point_absolute].isspace():
                slice_end_point_absolute += 1
            final_segments.append(sentence_text[current_search_offset:slice_end_point_absolute])
            current_search_offset = slice_end_point_absolute
        return final_segments

    def _merge_short_segments(self, segments: List[str]) -> List[str]:
        # ... (this function is unchanged) ...
        if not segments: return []
        word_counts = [count_real_words(s) for s in segments]
        while True:
            min_idx = -1
            for i, count in enumerate(word_counts):
                if 0 < count < self.min_segment_words:
                    min_idx = i
                    break
            if min_idx == -1: break
            can_merge_backward = min_idx > 0
            can_merge_forward = min_idx < len(segments) - 1
            merge_backward = False
            if not can_merge_backward and not can_merge_forward: break
            elif can_merge_backward and not can_merge_forward: merge_backward = True
            elif not can_merge_backward and can_merge_forward: merge_backward = False
            else:
                left_neighbor = segments[min_idx - 1].strip()
                right_neighbor_plus_one = segments[min_idx + 1].strip()
                if left_neighbor and left_neighbor[-1] in self.MERGE_FORWARD_PUNCT: merge_backward = False
                elif right_neighbor_plus_one and right_neighbor_plus_one[0] in self.OPENING_PUNCT: merge_backward = True
                else: merge_backward = word_counts[min_idx - 1] <= word_counts[min_idx + 1]
            if merge_backward:
                segments[min_idx - 1] += segments.pop(min_idx)
                word_counts[min_idx - 1] += word_counts.pop(min_idx)
            else:
                segments[min_idx] += segments.pop(min_idx + 1)
                word_counts[min_idx] += word_counts.pop(min_idx + 1)
        return [s for s in segments if s.strip()]

    def segment_sentence(self, sentence_text: str, s_id: str) -> List[str]:
        # ... (this function is unchanged) ...
        if not sentence_text or not sentence_text.strip(): return []
        initial_segments = self._get_initial_segments_from_llm(sentence_text, s_id)
        if len(initial_segments) <= 1: return initial_segments
        final_segments = self._merge_short_segments(initial_segments)
        return final_segments

class EnglishStanzaProcessor(LLMSegmenter):
    def __init__(self, config: Dict[str, Any], llm_logger: LLMLogger):
        super().__init__("English", config, llm_logger)

class SpanishStanzaProcessor(LLMSegmenter):
    def __init__(self, config: Dict[str, Any], llm_logger: LLMLogger):
        super().__init__("Spanish", config, llm_logger)