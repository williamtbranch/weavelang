# In llm2books/stages/finalize_simpler_adv_target.py

from typing import Any, Dict
from .base import SpaCyStage, logger
from .. import helper, validator # <-- IMPORT THE VALIDATOR

class FinalizeSimplerAdvTarget(SpaCyStage):
    """
    Stage 2: Processes the simpler_advanced_target tier with SpaCy,
    robustly reconstructs its full_text, and validates its own output.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=2, # This is the new stage number
            stage_name="FinalizeSimplerAdvTarget"
        )

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        lang_config = self.resources["language_config"]
        target_lang_code = lang_config["target_code"]
        spacy_target = self.resources["spacy_models"][target_lang_code]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                s_id = block.get("s_id", "Unknown")
                tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "simpler_advanced_target"), None)
                if not tier:
                    continue

                # --- STEP 1: Process segments and add lemmas (existing logic) ---
                all_tier_lemmas = set()
                diglot_idx_counter = 0
                segment_texts_for_reconstruction = []

                for seg in tier.get("segments", []):
                    raw_seg_text = seg.get("text", "")
                    segment_texts_for_reconstruction.append(raw_seg_text) # Store for later
                    
                    if not raw_seg_text.strip():
                        seg["tokenized_text"] = []
                        seg["lemmas"] = []
                        continue

                    seg_doc = spacy_target(raw_seg_text)
                    final_token_list = helper.create_golden_token_stream(seg_doc)
                    
                    seg_lemmas = set()
                    for token in final_token_list:
                        if token["t"] == "w":
                            token["di"] = diglot_idx_counter
                            diglot_idx_counter += 1
                            
                            # Simple lookup for lemma (context isn't critical here)
                            doc = spacy_target(token["v"])
                            main_token = next((t for t in doc if not t.is_punct and not t.is_space), None)
                            lemma_str = helper.normalize_spanish_lemma(main_token.lemma_ if main_token else token["v"])

                            if lemma_str:
                                token["l"] = [lemma_str]
                                all_tier_lemmas.add(lemma_str)
                                seg_lemmas.add(lemma_str)
                    
                    seg["tokenized_text"] = final_token_list
                    seg["lemmas"] = sorted(list(seg_lemmas))

                tier["lemmas"] = sorted(list(all_tier_lemmas))
                
                # --- STEP 2: ROBUSTLY RECONSTRUCT full_text ---
                # The full_text is now guaranteed to match the segments' content.
                tier["full_text"] = "".join(segment_texts_for_reconstruction)

                # --- STEP 3: VALIDATE AT THE SOURCE ---
                try:
                    logger.debug(f"S_ID {s_id}: Validating self-generated output for tier '{tier['tier_id']}'...")
                    validator.validate_segment_reconstruction(tier)
                    logger.debug(f"S_ID {s_id}: Self-validation PASSED.")
                except validator.ValidationError as e:
                    logger.error(f"FATAL: Stage '{self.stage_name}' created invalid data for S_ID {s_id}.")
                    # Re-raise the exception to halt the pipeline immediately
                    raise e

            block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        
        return data