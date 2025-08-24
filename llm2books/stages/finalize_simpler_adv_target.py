# In llm2books/stages/finalize_simpler_adv_target.py

from typing import Any, Dict
from .base import SpaCyStage
from .. import helper

class FinalizeSimplerAdvTarget(SpaCyStage):
    """
    New Stage 4: Processes the simpler_advanced_target tier with SpaCy.
    - Creates a definitive V2 token list for each segment.
    - Adds sequential, sentence-level `di` (diglot_index) keys to word tokens.
    - Populates the `lemmas` for the tier and each segment.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=4,
            stage_name="FinalizeSimplerAdvTarget"
        )

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        lang_config = self.resources["language_config"]
        target_lang_code = lang_config["target_code"]
        spacy_target = self.resources["spacy_models"][target_lang_code]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "simpler_advanced_target"), None)
                if not tier:
                    continue

                full_text = tier.get("full_text", "")
                if not full_text.strip():
                    tier["lemmas"] = []
                    continue

                full_doc = spacy_target(full_text)
                token_to_lemma = {
                    token.text: helper.normalize_spanish_lemma(token.lemma_)
                    for token in full_doc if not token.is_punct and not token.is_space
                }
                all_tier_lemmas = set()
                diglot_idx_counter = 0

                for seg in tier.get("segments", []):
                    raw_seg_text = seg.get("text", "")
                    if not raw_seg_text.strip():
                        seg["tokenized_text"] = []
                        seg["lemmas"] = []
                        continue

                    seg_doc = spacy_target(raw_seg_text)
                    final_token_list = helper.create_golden_token_stream(raw_seg_text, seg_doc)
                    
                    seg_lemmas = set()
                    for token in final_token_list:
                        if token["t"] == "w":
                            token["di"] = diglot_idx_counter
                            diglot_idx_counter += 1
                            
                            lemma_str = token_to_lemma.get(token["v"])
                            if lemma_str:
                                token["l"] = [lemma_str]
                                all_tier_lemmas.add(lemma_str)
                                seg_lemmas.add(lemma_str)
                    
                    seg["tokenized_text"] = final_token_list
                    seg["lemmas"] = sorted(list(seg_lemmas))

                tier["lemmas"] = sorted(list(all_tier_lemmas))

            block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        
        return data