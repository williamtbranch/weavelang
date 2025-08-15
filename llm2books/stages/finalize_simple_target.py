# llm2books/stages/finalize_simple_target.py
from typing import Any, Dict
from .base import SpaCyStage, logger
from .. import helper

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

                # Create one high-quality doc for the full text to ensure consistent lemmatization
                full_doc = spacy_target(full_text)
                token_to_lemma = {
                    token.text: helper.normalize_spanish_lemma(token.lemma_)
                    for token in full_doc if not token.is_punct and not token.is_space
                }

                all_tier_lemmas = set()

                for seg in simple_target_tier.get("segments", []):
                    # Get the raw text from the 'text' field populated in the previous stage
                    raw_seg_text = seg.get("text", "")
                    seg_doc = spacy_target(raw_seg_text)

                    # Use the canonical helper to create the final BWBWB token list
                    final_token_list = helper.create_v2_token_list(seg_doc[:])

                    # Add lemmas to the new token list using our high-quality map
                    for token in final_token_list:
                        if token["t"] == "w":
                            lemma_str = token_to_lemma.get(token["v"])
                            if lemma_str:
                                token["l"] = [lemma_str]
                                all_tier_lemmas.add(lemma_str)
                    
                    # Replace the placeholder tokenized_text from Stage 2 with this final, rich version
                    seg["tokenized_text"] = final_token_list

                # Set the aggregated list of unique, sorted lemmas for the entire tier
                simple_target_tier["lemmas"] = sorted(list(all_tier_lemmas))
            
            block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        
        return data