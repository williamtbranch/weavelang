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
                tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "simple_target"), None)
                if not tier:
                    continue

                # The full_text from the previous stage might be wrong, so we will rebuild it.
                all_tier_lemmas = set()
                
                for seg in tier.get("segments", []):
                    raw_seg_text = seg.get("text", "")
                    if not raw_seg_text.strip():
                        seg["tokenized_text"] = []
                        seg["lemmas"] = []
                        continue

                    seg_doc = spacy_target(raw_seg_text)
                    final_token_list = helper.create_golden_token_stream(seg_doc)
                    
                    seg_lemmas = set()
                    for token in final_token_list:
                        if token["t"] == "w":
                            # No 'di' keys needed for this tier
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
                
                # --- THIS IS THE FIX ---
                # After all segments are processed and tokenized, reconstruct the full_text
                # from the gold-standard token streams to ensure perfect spacing.
                reconstructed_full_text = "".join(
                    token['v'] 
                    for seg in tier['segments'] 
                    for token in seg['tokenized_text']
                )
                tier['full_text'] = reconstructed_full_text
                # --- END OF FIX ---

            block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"

        return data