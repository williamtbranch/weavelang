# llm2books/stages/process_target_tiers.py
from typing import Any, Dict
from .base import SpaCyStage, logger
from .. import helper, validator

class ProcessTargetTiers(SpaCyStage):
    """
    Stage 2: A unified stage to perform SpaCy processing (tokenization,
    lemmatization) on all four target-language tiers. It also adds the
    critical 'di' key to the simple_target tier.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=2,
            stage_name="ProcessTargetTiers"
        )

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        lang_config = self.resources["language_config"]
        target_lang_code = lang_config["target_code"]
        spacy_target = self.resources["spacy_models"][target_lang_code]

        tiers_to_process = [
            "advanced_target",
            "moderate_target",
            "basic_target",
            "simple_target",
        ]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                s_id = block.get("s_id", "Unknown")
                
                # --- FIX: Initialize a sentence-wide counter for diglot indices ---
                diglot_idx_counter = 0

                for tier_id in tiers_to_process:
                    tier = next((t for t in block.get("tiers", []) if t["tier_id"] == tier_id), None)
                    if not tier:
                        logger.warning(f"S_ID {s_id}: Tier '{tier_id}' not found. Skipping processing for it.")
                        continue

                    try:
                        # --- FIX: Pass the counter to the processing function ---
                        # The function will return the new value of the counter.
                        diglot_idx_counter = self._process_single_tier(tier, spacy_target, diglot_idx_counter)
                        
                        validator.validate_segment_reconstruction(tier)
                    except validator.ValidationError as e:
                        logger.error(f"FATAL: Stage '{self.stage_name}' created invalid data for S_ID {s_id}, Tier '{tier_id}'.")
                        raise e

                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        
        return data

    def _process_single_tier(self, tier: Dict[str, Any], spacy_model: Any, diglot_idx_counter: int) -> int:
        """
        Processes a single tier dictionary in-place and returns the updated diglot index counter.
        """
        tier_id = tier["tier_id"]
        segment_texts = [seg.get("text", "") for seg in tier.get("segments", [])]
        tier["full_text"] = "".join(segment_texts)

        if not tier["full_text"].strip():
            tier["lemmas"] = []
            for seg in tier.get("segments", []):
                seg["lemmas"] = []
                seg["tokenized_text"] = []
            return diglot_idx_counter

        full_doc = spacy_model(tier["full_text"])
        token_to_lemma_map = {
            t.text: helper.normalize_spanish_lemma(t.lemma_)
            for t in full_doc if not t.is_punct and not t.is_space
        }

        all_tier_lemmas = set()
        for seg in tier.get("segments", []):
            seg_text = seg.get("text", "")
            if not seg_text.strip():
                seg["lemmas"] = []
                seg["tokenized_text"] = []
                continue

            seg_doc = spacy_model(seg_text)
            seg_tokens = helper.create_golden_token_stream(seg_doc)
            
            seg_lemmas = set()
            for token in seg_tokens:
                if token["t"] == "w":
                    # --- FIX: Add 'di' keys ONLY for the simple_target tier ---
                    if tier_id == "simple_target":
                        token["di"] = diglot_idx_counter
                        diglot_idx_counter += 1

                    lemma = token_to_lemma_map.get(token["v"])
                    if lemma:
                        token["l"] = [lemma]
                        seg_lemmas.add(lemma)
            
            seg["tokenized_text"] = seg_tokens
            seg["lemmas"] = sorted(list(seg_lemmas))
            all_tier_lemmas.update(seg_lemmas)
        
        tier["lemmas"] = sorted(list(all_tier_lemmas))
        
        # Return the final state of the counter for the next tier
        return diglot_idx_counter