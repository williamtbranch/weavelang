import re
from typing import Any, Dict, List, Optional

from .base import LLMStage
from .. import llm_prompts


class GenerateDiglotMap(LLMStage):
    """
    Stage 7: Generates a word-for-word mapping (Diglot Map) from English to
    simple Spanish using an LLM.
    """

    def __init__(self, book_stem: str, config: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            config=config,
            common_resources=common_resources,
            stage_number=7,
            stage_name="GenerateDiglotMap",
            batch_size=1,  # Process one full sentence (all its segments) at a time
            parser_type="block",
        )

    # --- LLMStage Abstract Method Implementations ---

    def get_system_prompt(self) -> str:
        """Loads and returns the system prompt for generating the diglot map."""
        return llm_prompts.load_prompt_template("new_stage7_diglot_prompt.txt")

    def prepare_llm_input(
        self, block: Dict[str, Any], s_idx: int
    ) -> Optional[List[Dict[str, Any]]]:
        alignments = block.get("phrase_alignments_l3_to_english", [])
        if not alignments:
            return None

        prepared_items = []
        s_id_num = block["original_sentence_s_id"].replace("S", "")

        for align in alignments:
            english_text = align.get("english_span_text", "")
            if english_text.strip():
                cleaned_eng_text = re.sub(r"[^\w\s-]", "", english_text).strip()
                if not cleaned_eng_text:
                    continue
                
                llm_id = f"id {s_id_num}_{align['segment_id']}".lower()
                prepared_items.append({
                    "llm_id": llm_id,
                    "prompt_text": f"{llm_id}:\n{cleaned_eng_text}"
                })

        return prepared_items

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