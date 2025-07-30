# llm2books/stages/finalize_simpler_target.py
from typing import Any, Dict
from .base import SpaCyStage, logger
from .. import helper

class FinalizeSimplerTarget(SpaCyStage):
    """
    Stage 4 (V2): Tokenizes and lemmatizes both the advanced and simpler-advanced
    target language tiers, populating their final V2 token structures.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(book_stem, cli_args, common_resources, stage_number=4, stage_name="FinalizeSimplerTarget")

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        lang_config = self.resources["language_config"]
        target_lang_code = lang_config["target_code"]
        spacy_target = self.resources["spacy_models"][target_lang_code]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                self._process_tier(block, "advanced_target", spacy_target)
                self._process_tier(block, "simpler_advanced_target", spacy_target)

            block.setdefault("llm_call_status", {})[f"stage{self.stage_number}"] = "COMPLETED_SPACY"
        return data

    def _process_tier(self, block: Dict[str, Any], tier_id: str, spacy_model):
        """Helper to tokenize and lemmatize a single tier's data."""
        tier = next((t for t in block.get("tiers", []) if t["tier_id"] == tier_id), None)
        if not tier: return

        full_text = tier.get("full_text", "")
        if not full_text.strip(): return
        
        full_doc = spacy_model(full_text)
        token_to_lemma = {
            token.text: helper.normalize_spanish_lemma(token.lemma_)
            for token in full_doc if not token.is_punct and not token.is_space
        }

        all_tier_lemmas = set()
        for seg in tier.get("segments", []):
            # Reconstruct text from the placeholder
            raw_seg_text = "".join(t.get("v", "") for t in seg.get("tokenized_text", []))
            seg_doc = spacy_model(raw_seg_text)
            final_token_list = helper.create_v2_token_list(raw_seg_text, seg_doc)

            for token in final_token_list:
                if token["t"] == "w":
                    lemma_str = token_to_lemma.get(token["v"])
                    if lemma_str:
                        token["l"] = [lemma_str]
                        all_tier_lemmas.add(lemma_str)
            
            seg["tokenized_text"] = final_token_list
        
        tier["lemmas"] = sorted(list(all_tier_lemmas))