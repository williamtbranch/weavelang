# llm2books/stages/lemmatize_advanced_target.py
from typing import Any, Dict
from .base import SpaCyStage, logger
from .. import helper

class LemmatizeAdvancedTarget(SpaCyStage):
    """Stage 2 (V2): Lemmatizes the 'full_text' of the 'advanced_target' tier."""
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(book_stem, cli_args, common_resources, stage_number=2, stage_name="LemmatizeAdvancedTarget")

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        lang_config = self.resources["language_config"]
        target_lang_code = lang_config["target_code"]
        spacy_target = self.resources["spacy_models"][target_lang_code]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                adv_target_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "advanced_target"), None)
                if not adv_target_tier: continue

                source_text = adv_target_tier.get("full_text", "")
                if source_text.strip():
                    doc = spacy_target(source_text)
                    raw_lemmas = [token.lemma_ for token in doc if not token.is_punct and not token.is_space and token.pos_ != "PROPN"]
                    # In V2, lemmas will be stored as strings in the JSON for portability,
                    # and converted to IDs in the Rust preprocessor.
                    adv_target_tier["lemmas"] = sorted(list(set(
                        norm_lemma for s in raw_lemmas if (norm_lemma := helper.normalize_spanish_lemma(s))
                    )))
                else:
                    adv_target_tier["lemmas"] = []

            block.setdefault("llm_call_status", {})[f"stage{self.stage_number}"] = "COMPLETED_SPACY"
        return data