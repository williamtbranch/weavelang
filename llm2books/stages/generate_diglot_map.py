import re
from typing import Any, Dict, List, Optional, Tuple
from .base import LLMStage
from .. import llm_prompts


class GenerateDiglotMap(LLMStage):
    """
    Stage 7: Generates a word-for-word mapping (Diglot Map) from English to
    simple Spanish using an LLM.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=7,
            stage_name="GenerateDiglotMap",
            parser_type="block",
        )

    # --- LLMStage Abstract Method Implementations ---

    def get_system_prompt(self) -> str:
        """Loads and returns the system prompt for generating the diglot map."""
        return llm_prompts.load_prompt_template("new_stage7_diglot_prompt.txt")

    def prepare_atomic_unit(self, block: Dict[str, Any]) -> Tuple[Optional[List[Dict[str, Any]]], int]:
        """
        Prepares all English segments from one sentence as an atomic unit for diglot mapping.
        Returns the prompt parts and the total estimated tokens for this unit.
        """
        alignments = block.get("phrase_alignments_l3_to_english", [])
        if not alignments:
            return None, 0

        prepared_items = []
        full_prompt_text_for_unit = []
        s_id_num = block["original_sentence_s_id"].replace("S", "")

        for align in alignments:
            english_text = align.get("english_span_text", "")
            if not english_text.strip():
                continue

            # Clean punctuation from the text before sending to the LLM
            cleaned_eng_text = re.sub(r"[^\w\s-]", "", english_text).strip()
            if not cleaned_eng_text:
                continue
            
            llm_id = f"id {s_id_num}_{align['segment_id']}".lower()
            prompt_block = f"{llm_id}:\n{cleaned_eng_text}"
            
            prepared_items.append({
                "llm_id": llm_id,
                "prompt_text": prompt_block
            })
            full_prompt_text_for_unit.append(prompt_block)

        if not prepared_items:
            return None, 0
            
        token_estimate = self._estimate_tokens("\n".join(full_prompt_text_for_unit))
        
        return prepared_items, token_estimate
    def process_llm_response(
        self, block: Dict[str, Any], llm_response: Dict[str, str]
    ) -> None:
        final_diglot_entries: List[Dict[str, Any]] = []
        mapping_regex = re.compile(r"^\s*([^->]+?)\s*->\s*(.+)$")

        for align in block.get("phrase_alignments_l3_to_english", []):
            lookup_id = f"id {block['original_sentence_s_id'].replace('S', '')}_{align['segment_id']}".lower()
            
            # llm_response now contains the mapping block for just this segment
            segment_mapping_text = llm_response.get(lookup_id, "")

            for line in segment_mapping_text.splitlines():
                map_match = mapping_regex.match(line.strip())
                if map_match:
                    eng_word = map_match.group(1).strip()
                    spa_form = map_match.group(2).strip()
                    
                    note, is_viable = (spa_form, True)
                    if spa_form in ["PROPER_NOUN", "NO_SUB"]:
                        is_viable = False
                    else:
                        note = "viable"

                    final_diglot_entries.append({
                        "segment_id": align["segment_id"],
                        "english_word": eng_word,
                        "spanish_lemma": "",
                        "exact_spanish_form": spa_form,
                        "is_viable_for_substitution": is_viable,
                        "note": note,
                    })

        block["diglot_map_entries"] = final_diglot_entries
        status_key = f"stage{self.stage_number}"
        block.setdefault("llm_call_status", {})[status_key] = "COMPLETED_LLM"