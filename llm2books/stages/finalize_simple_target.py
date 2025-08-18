# llm2books/stages/finalize_simple_target.py
from typing import Any, Dict
from .base import SpaCyStage, logger
from .. import helper
from .. import validator

class FinalizeSimpleTarget(SpaCyStage):
    """
    Stage 3: Processes the simple_target tier with SpaCy to perform definitive
    tokenization and lemmatization, creating the final V2 token structure.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=3,
            stage_name="FinalizeSimpleTarget"
        )

    #
    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        lang_config = self.resources["language_config"]
        target_lang_code = lang_config["target_code"]
        spacy_target = self.resources["spacy_models"][target_lang_code]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                simple_target_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "simple_target"), None)
                if not simple_target_tier:
                    continue

                full_text = simple_target_tier.get("full_text", "")
                if not full_text.strip():
                    simple_target_tier["lemmas"] = []
                    continue

                # --- THIS IS THE MISSING SETUP LOGIC ---
                full_doc = spacy_target(full_text)
                token_to_lemma = {
                    token.text: helper.normalize_spanish_lemma(token.lemma_)
                    for token in full_doc if not token.is_punct and not token.is_space
                }
                all_tier_lemmas = set()
                # Initialize the counter for the whole tier
                diglot_idx_counter = 0 
                # --- END OF MISSING SETUP LOGIC ---

                for seg in simple_target_tier.get("segments", []):
                    raw_seg_text = seg.get("text", "")
                    seg_doc = spacy_target(raw_seg_text)
                    final_token_list = helper.create_golden_token_stream(raw_seg_text, seg_doc)
                    
                    # This block of code can now find the variables it needs
                    seg_lemmas = set()
                    for token in final_token_list:
                        if token["t"] == "w":
                            # The 'di' key is for base/simple alignment.
                            # It is not technically part of the final schema for target tiers,
                            # but we can add it for now if needed for intermediate steps.
                            # Let's remove it for now to be cleaner.
                            # token["di"] = diglot_idx_counter 
                            # diglot_idx_counter += 1
                            lemma_str = token_to_lemma.get(token["v"])
                            if lemma_str:
                                token["l"] = [lemma_str]
                                all_tier_lemmas.add(lemma_str)
                                seg_lemmas.add(lemma_str)
                    
                    seg["tokenized_text"] = final_token_list
                    seg["lemmas"] = sorted(list(seg_lemmas))

                simple_target_tier["lemmas"] = sorted(list(all_tier_lemmas))
            
            block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"

        
        # --- (Validation will be added in the next step) ---
        logger.info("      -> Validating generated simple_target tier...")
        try:
            for block in data.get("content_blocks", []):
                if block.get("block_type") == "sentence":
                    simple_target_tier = next((t for t in block["tiers"] if t["tier_id"] == "simple_target"), None)
                    if simple_target_tier:
                        # Rule #3: Segments -> full_text
                        validator.validate_segment_reconstruction(simple_target_tier)
                        for seg in simple_target_tier.get("segments", []):
                            # Rule #2: Tokens -> segment.text
                            reconstructed_from_tokens = "".join(t['v'] for t in seg.get("tokenized_text", []))
                            if reconstructed_from_tokens != seg.get("text"):
                                raise validator.ValidationError(f"Token reconstruction for s_id {block['s_id']} seg_id {seg['seg_id']} failed.")
        except validator.ValidationError as e:
            # Raise the error again to be caught by the stage's run method
            raise e
        return data