# llm2books/stages/simplify_adv_spanish.py

from typing import Any, Dict, List, Optional, Tuple, Set
from .base import LLMStage, logger
from .. import llm_prompts, helper
import time

# --- NEW CONFIGURATION ---
VOCAB_CEILING = 5000  # The max rank allowed for a lemma
MAX_REPAIR_ATTEMPTS = 3 # Max number of times to re-prompt the LLM for a single batch

class SimplifyAdvSpanish(LLMStage):
    """
    Stage 3b: Simplifies the vocabulary of each advanced Spanish segment using an LLM,
    now with a vocabulary ceiling validation and a repair loop.
    """

    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=3,
            stage_name="SimplifyAdvSpanish",
            parser_type="line",
        )
        # --- NEW: Load frequency list once for the stage ---
        self.lemma_to_rank = self._load_frequency_list()

    def _load_frequency_list(self) -> Dict[str, int]:
        """Loads the master frequency list into memory."""
        freq_list_path_str = self.resources.get("pipeline_config", {}).get("frequency_list_path", "assets/es_master_frequency_list.txt")
        freq_list_path = self.content_project_root / freq_list_path_str
        
        if not freq_list_path.is_file():
            logger.critical(f"CRITICAL: Frequency list not found at '{freq_list_path}' for validation.")
            return {} # Return empty dict to prevent crash, but validation will fail.

        lemma_to_rank = {}
        try:
            with open(freq_list_path, "r", encoding="utf-8") as f:
                next(f)  # Skip header
                for line in f:
                    parts = line.strip().split("\t")
                    if len(parts) >= 2:
                        lemma = helper.normalize_spanish_lemma(parts[0])
                        if lemma:
                            lemma_to_rank[lemma] = int(parts[1])
        except (IOError, ValueError) as e:
            logger.error(f"Error loading frequency list: {e}")
        
        logger.info(f"      -> Loaded {len(lemma_to_rank)} lemmas for vocabulary validation.")
        return lemma_to_rank

    def get_system_prompt(self) -> str:
        """Loads the NEW system prompt for simplifying segments."""
        # This now points to your new, improved prompt file
        return llm_prompts.load_prompt_template("new_stage3_simplifier_prompt.txt")

    def prepare_atomic_unit(self, block: Dict[str, Any]) -> Tuple[Optional[List[Dict[str, Any]]], int]:
        # ... (this method remains exactly the same as before) ...
        segments = block.get("adv_spanish_segments", [])
        if not segments:
            return None, 0

        prepared_items = []
        full_prompt_text_for_unit = []
        s_id_num = block["original_sentence_s_id"].replace("S", "")

        for seg in segments:
            llm_id = f"id {s_id_num}_{seg['segment_id']}".lower()
            prompt_line = f"{llm_id}: {seg['advanced_text']}"
            
            prepared_items.append({
                "llm_id": llm_id,
                "prompt_text": prompt_line
            })
            full_prompt_text_for_unit.append(prompt_line)

        token_estimate = self._estimate_tokens("\n".join(full_prompt_text_for_unit))
        return prepared_items, token_estimate

    def process_llm_response(self, block: Dict[str, Any], llm_response: Dict[str, str]) -> None:
        # This method is now a bit simpler; it just populates the initial text.
        # The validation happens at the batch level.
        s_id_num = block["original_sentence_s_id"].replace("S", "")
        for seg in block.get("adv_spanish_segments", []):
            lookup_id = f"id {s_id_num}_{seg['segment_id']}".lower()
            simpler_text = llm_response.get(lookup_id, seg.get("advanced_text", ""))
            seg["simpler_text"] = simpler_text

    def _process_batch(
        self, batch_units: List[Dict[str, Any]], data: Dict[str, Any]
    ) -> bool:
        """
        Overridden _process_batch to include the validation and repair loop.
        """
        self.batch_counter += 1
        logger.info(
            f"      -> Processing batch #{self.batch_counter} with {len(batch_units)} atomic units..."
        )
        
        # --- INITIAL LLM CALL ---
        prompt_parts = []
        expected_ids = []
        for unit_info in batch_units:
            for part in unit_info['prompt_parts']:
                prompt_parts.append(part["prompt_text"])
                expected_ids.append(part["llm_id"])

        user_prompt = "\n".join(prompt_parts)
        self._write_batch_header_to_log(user_prompt)
        
        parsed_data = self._make_api_call_with_retries(user_prompt, expected_ids)
        if parsed_data is None:
            self._save_progress(data, "FAILED")
            return False

        # --- VALIDATION AND REPAIR LOOP ---
        spacy_es = self.resources.get("spacy_models", {}).get("es")
        if not spacy_es:
            logger.critical("Spanish SpaCy model not found, cannot validate.")
            return False

        for repair_attempt in range(MAX_REPAIR_ATTEMPTS + 1):
            failing_segments = []
            forbidden_words = set()

            # Validate the current `parsed_data`
            for llm_id, simple_text in parsed_data.items():
                doc = spacy_es(simple_text)
                for token in doc:
                    if not token.is_punct and not token.is_space:
                        norm_lemma = helper.normalize_spanish_lemma(token.lemma_)
                        if not norm_lemma: continue
                        
                        rank = self.lemma_to_rank.get(norm_lemma, float('inf'))
                        if rank > VOCAB_CEILING:
                            failing_segments.append(llm_id)
                            forbidden_words.add(norm_lemma)

            if not failing_segments:
                logger.info(f"        -> Batch #{self.batch_counter} PASSED validation.")
                break # Success! Exit the repair loop.
            
            if repair_attempt >= MAX_REPAIR_ATTEMPTS:
                logger.error(f"      -> Batch #{self.batch_counter} FAILED validation after {MAX_REPAIR_ATTEMPTS} repair attempts. Halting.")
                # You might want to write an error file here.
                return False

            logger.warning(f"      -> Batch #{self.batch_counter} failed validation (Attempt {repair_attempt + 1}/{MAX_REPAIR_ATTEMPTS}). Found {len(forbidden_words)} out-of-bounds words.")
            logger.warning(f"        -> Forbidden words: {', '.join(sorted(list(forbidden_words)))}")
            
            # --- CONSTRUCT AND RUN REPAIR CALL ---
            # We only re-prompt for the unique failing segments
            failing_ids = sorted(list(set(failing_segments)))
            repair_prompt_parts = [p for p in prompt_parts if any(fid in p for fid in failing_ids)]
            
            repair_user_prompt = self._construct_repair_prompt(repair_prompt_parts, forbidden_words)
            
            logger.info(f"        -> Re-prompting for {len(failing_ids)} failing segments...")
            repair_parsed_data = self._make_api_call_with_retries(repair_user_prompt, failing_ids, use_fallback=True)
            
            if repair_parsed_data is None:
                logger.error("      -> Repair LLM call failed. Halting.")
                return False
            
            # Update the main `parsed_data` with the repaired content
            parsed_data.update(repair_parsed_data)

        # --- FINAL PROCESSING ---
        # If we get here, the batch is validated.
        for unit_info in batch_units:
            self.process_llm_response(unit_info['unit_data'], parsed_data)
        
        try:
            with open(self.log_path, "a", encoding="utf-8") as f:
                f.write(f"--- END OF BATCH {self.batch_counter} ---\n\n")
        except IOError as e:
            logger.warning(f"      -> Could not write batch footer to log file: {e}")

        return self._save_progress(data, "PARTIAL_3B_LLM_COMPLETE")

    def _construct_repair_prompt(self, failing_prompt_parts: List[str], forbidden_words: Set[str]) -> str:
        """Generates the user prompt for a repair attempt."""
        # This could also be a template file, but inline is fine for this.
        header = (
            "You are a Spanish linguist correcting a previous translation that used words that are too advanced.\n"
            "You must rephrase the following segments to be simpler.\n"
            "\n**CRITICAL RULE:** Do NOT use any of the following forbidden words in your new response.\n"
        )
        forbidden_list = "- " + "\n- ".join(sorted(list(forbidden_words)))
        body = "\n**PHRASES TO FIX:**\n" + "\n".join(failing_prompt_parts)
        
        return f"{header}\n**FORBIDDEN WORDS:**\n{forbidden_list}\n{body}"

    def _make_api_call_with_retries(
        self, user_prompt: str, expected_ids: List[str], use_fallback: bool = False
    ) -> Optional[Dict[str, str]]:
        """A more focused version of the base class method for this stage's needs."""
        # This method can call the base class's _attempt_model or reimplement a simpler version
        # For simplicity, we'll re-use the base logic.
        
        system_prompt = self.get_system_prompt() if not use_fallback else self._construct_repair_prompt([], set())
        
        parser_func = self._parse_llm_response_line
        
        primary_model_key = self.stage_config.get("primary_model")
        fallback_model_key = self.stage_config.get("fallback_model")
        
        model_to_use_key = fallback_model_key if use_fallback and fallback_model_key else primary_model_key
        model_tier = "fallback" if use_fallback and fallback_model_key else "primary"
        
        parsed_data = self._attempt_model(system_prompt, user_prompt, model_to_use_key, model_tier, parser_func, expected_ids)
        
        if parsed_data is None and not use_fallback and fallback_model_key:
             logger.warning(f"      -> Primary model failed. Escalating to fallback model '{fallback_model_key}'")
             parsed_data = self._attempt_model(system_prompt, user_prompt, fallback_model_key, "fallback", parser_func, expected_ids)

        return parsed_data