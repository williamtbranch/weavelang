# llm2books/stages/lemmatize_inverse_diglot_map.py
from typing import Any, Dict
from .base import SpaCyStage, logger
from .. import helper

def reconstruct_text_from_tokens(tokens: list) -> str:
    """Helper to reconstruct a plain text string from a V2 token list."""
    return "".join(token.get("v", "") for token in tokens)

class LemmatizeInverseDiglotMap(SpaCyStage):
    """
    Stage 10 (V2): Lemmatizes the target words in the inverse diglot map.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(book_stem, cli_args, common_resources, stage_number=10, stage_name="LemmatizeInverseDiglotMap")

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        lang_config = self.resources["language_config"]
        target_lang_code = lang_config["target_code"]
        spacy_target = self.resources["spacy_models"][target_lang_code]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                inv_diglot_map = block.get("mappings", {}).get("adv_target_to_base_inv_diglot", {})
                simpler_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "simpler_advanced_target"), None)
                if not simpler_tier or not inv_diglot_map: continue

                for seg_id, entries in inv_diglot_map.items():
                    # Find the corresponding segment to get the original word
                    seg = next((s for s in simpler_tier.get("segments", []) if s["seg_id"] == seg_id), None)
                    if not seg: continue
                    
                    target_word_tokens = [tok for tok in seg.get("tokenized_text", []) if tok["t"] == "w"]

                    for entry in entries:
                        # entry is [target_word_index, 0, "base_substitute"]
                        word_index = entry[0]
                        
                        target_word = target_word_tokens[word_index].get("v") if word_index < len(target_word_tokens) else None
                        
                        lemma_str = ""
                        if target_word:
                            doc = spacy_target(target_word)
                            main_token = next((t for t in doc if not t.is_punct), None)
                            if main_token:
                                lemma_str = helper.normalize_spanish_lemma(main_token.lemma_)
                            else:
                                lemma_str = helper.normalize_spanish_lemma(target_word)

                        # Replace the placeholder 0 with the lemma string
                        entry[1] = lemma_str

            block.setdefault("llm_call_status", {})[f"stage{self.stage_number}"] = "COMPLETED_SPACY"
        return data