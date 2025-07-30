# llm2books/stages/generate_simple_target.py

from typing import Any, Dict, List, Optional, Tuple
from .base import LLMStage, logger
from .. import llm_prompts

def reconstruct_text_from_tokens(tokens: List[Dict[str, Any]]) -> str:
    """Helper to reconstruct a plain text string from a V2 token list."""
    return "".join(token.get("v", "") for token in tokens)

class GenerateSimpleTarget(LLMStage):
    """
    Stage 5b (V2): Translates base language segments into simple target language.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=5,
            stage_name="GenerateSimpleTarget",
            parser_type="line",
        )

    def get_system_prompt(self) -> str:
        """Loads and returns the system prompt for this stage."""
        prompt_dir = self.resources["language_config"]["prompt_dir"]
        # The stage_name "GenerateSimpleTarget" is used as the key
        return llm_prompts.load_prompt_template(self.stage_name, prompt_dir)

    def prepare_atomic_unit(self, block: Dict[str, Any]) -> Tuple[Optional[List[Dict[str, Any]]], int]:
        """Prepares all base language segments from one sentence for the LLM."""
        base_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "base"), None)
        if not base_tier or not base_tier.get("segments"):
            return None, 0

        prepared_items = []
        full_prompt_text_for_unit = []
        s_id_num = block["s_id"].replace("S", "")

        for seg in base_tier["segments"]:
            # Reconstruct the plain text from the tokenized version
            base_text = reconstruct_text_from_tokens(seg.get("tokenized_text", []))
            if not base_text.strip():
                continue

            llm_id = f"id {s_id_num}_{seg['seg_id']}".lower()
            prompt_line = f"{llm_id}: {base_text}"
            
            prepared_items.append({
                "llm_id": llm_id,
                "prompt_text": prompt_line,
                "seg_id": seg['seg_id'] # Pass seg_id for response processing
            })
            full_prompt_text_for_unit.append(prompt_line)

        token_estimate = self._estimate_tokens("\n".join(full_prompt_text_for_unit))
        return prepared_items, token_estimate

    def process_llm_response(self, block: Dict[str, Any], llm_response: Dict[str, str]) -> None:
        """
        Creates the 'simple_target' tier and populates it with the LLM's
        raw text output. Tokenization will happen in the next stage.
        """
        s_id_num = block["s_id"].replace("S", "")
        
        # Find or create the simple_target tier
        simple_target_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "simple_target"), None)
        if not simple_target_tier:
            simple_target_tier = {"tier_id": "simple_target", "segments": [], "lemmas": []}
            block.setdefault("tiers", []).append(simple_target_tier)

        # Get the segments from the base tier to know what to create
        base_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "base"), {})
        
        all_simple_texts = []
        for seg_data in base_tier.get("segments", []):
            seg_id = seg_data["seg_id"]
            lookup_id = f"id {s_id_num}_{seg_id}".lower()
            
            # Get the translated text from the LLM response
            simple_text = llm_response.get(lookup_id, "")
            all_simple_texts.append(simple_text)

            # Create a new segment in the simple_target tier with the raw text
            simple_target_tier["segments"].append({
                "seg_id": seg_id,
                "post_separator": seg_data.get("post_separator", ""), # Copy from base
                "tokenized_text": [
                    # This is a temporary placeholder. Stage 6 will overwrite it.
                    {"t": "b", "v": ""},
                    {"t": "w", "v": simple_text}
                ]
            })

        # Populate the full_text for the tier for integrity checks
        simple_target_tier["full_text"] = "".join(
            seg.get("post_separator", "") + reconstruct_text_from_tokens(seg.get("tokenized_text", []))
            for seg in simple_target_tier["segments"]
        ).lstrip()

        status_key = f"stage{self.stage_number}b"
        block.setdefault("llm_call_status", {})[status_key] = "COMPLETED_LLM"