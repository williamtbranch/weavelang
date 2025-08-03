# llm2books/stages/initialize_book_tiers.py

import re
from typing import Any, Dict, List, Optional, Tuple

from .base import LLMStage, logger
from .. import llm_prompts, helper


class InitializeBookTiers(LLMStage):
    """
    Stage 1: Initializes the foundational JSON structure for a book.

    It reads a source text file (of any configured language) and ensures that
    the output JSON has both the `base` and `advanced_target` tiers populated
    with their full text. It uses a "Universal Source" model, making 0, 1, or 2
    LLM calls as needed to generate any missing text.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=1,
            stage_name="InitializeBookTiers",
            parser_type='line'
        )
        # These are set by the orchestrator
        self.source_path = self.resources['source_path']
        self.source_lang = self.resources['source_lang']
        self.lang_config = self.resources['language_config']

    def get_system_prompt(self, from_lang: str = None, to_lang: str = None) -> str:
        """
        Loads the system prompt for translation.
        This is a placeholder and will be made more dynamic.
        """
        # This logic is now part of _generate_text_via_llm, so we provide a dummy
        return "You are a translator."

    def prepare_atomic_unit(self, block: Dict[str, Any]) -> Tuple[Optional[List[Dict[str, Any]]], int]:
        """Not used by this stage's custom run() method."""
        pass

    def process_llm_response(self, block: Dict[str, Any], llm_response: Dict[str, str]) -> None:
        """Not used by this stage's custom run() method."""
        pass

    def _parse_source_file(self) -> List[Dict[str, Any]]:
        """Parses the raw source .txt file into a list of chapter/sentence dicts."""
        raw_lines = self.source_path.read_text(encoding="utf-8").splitlines()
        # Skip the first line, which is the %%lang:xx%% tag
        content_lines = raw_lines[1:]
        
        all_items = []
        sentence_regex = re.compile(r"^{S(\d+):\s*(.*)}$")
        chapter_regex = re.compile(r"^%%CHAPTER_MARKER%%\s*(.*)$")
        
        s_id_counter = 1
        for line in content_lines:
            line = line.strip()
            if not line:
                continue
            if chapter_match := chapter_regex.match(line):
                all_items.append({"type": "chapter", "text": chapter_match.group(1).strip()})
            elif sentence_match := sentence_regex.match(line):
                s_id_val = int(sentence_match.group(1))
                all_items.append({
                    "type": "sentence",
                    "s_id": f"S{s_id_val}",
                    "text": sentence_match.group(2).strip(),
                })
            else: # Handle plain text files without {S...} markers
                all_items.append({
                    "type": "sentence",
                    "s_id": f"S{s_id_counter}",
                    "text": line
                })
                s_id_counter += 1

        return all_items

    def _generate_text_via_llm(self, source_items: List[Dict], to_lang_code: str) -> Optional[Dict[str, str]]:
        """
        Generic helper to call the LLM for translation.
        """
        from_lang = self.source_lang
        manifest = self.lang_config['manifest']
        
        # --- CORRECT PROMPT DIRECTORY LOGIC ---
        # This block correctly determines which prompt directory to use for any
        # translation direction (forward, reverse, or third-party).
        if from_lang == self.lang_config['base_code'] and to_lang_code == self.lang_config['target_code']:
            # This is the primary "forward" run (e.g., en -> es). Use the pre-calculated pair_prompt_dir.
            pair_prompt_dir = self.lang_config.get('pair_prompt_dir')
        else:
            # This is a "reverse" (e.g., es -> en) or "third-party" (e.g., it -> en) run.
            # We must construct the pair key and look it up in the manifest.
            pair_key = f"{from_lang}-{to_lang_code}"
            pair_prompt_dir = manifest.get('pair', {}).get(pair_key, {}).get('prompt_dir')
        
        # --- LOAD PROMPT TEMPLATE ---
        try:
            prompt_template = llm_prompts.load_prompt_template(
                prompt_name="stage1_translate.txt",
                base_asset_path=self.resources['tool_root_dir'] / "assets",
                pair_prompt_dir=pair_prompt_dir
            )
        except FileNotFoundError as e:
            logger.critical(f"Could not load prompt for translation from '{from_lang}' to '{to_lang_code}': {e}")
            return None

        # --- CORRECT LANGUAGE NAME LOOKUP ---
        from_lang_name = manifest.get(from_lang, {}).get('name', from_lang)
        to_lang_name = manifest.get(to_lang_code, {}).get('name', to_lang_code)
        
        sentences_for_prompt = "\n".join([
            f"id {item['s_id'].replace('S', '')}: {item['text']}" 
            for item in source_items if item['type'] == 'sentence'
        ])
        
        # Populate the prompt template
        system_prompt = prompt_template.format(
            source_language_name=from_lang_name,
            target_language_name=to_lang_name,
            batched_input_sentences=sentences_for_prompt
        )

        user_prompt = "Please proceed with the translation based on the rules and batch provided."
        expected_ids = [f"id {item['s_id'].replace('S', '')}" for item in source_items if item['type'] == 'sentence']

        # Call the base class's API method
        # NOTE: We need to pass the system_prompt directly now.
        parsed_data = self._make_api_call_with_retries(
            user_prompt=user_prompt, 
            expected_ids=expected_ids, 
            system_prompt_override=system_prompt
        )
        
        if parsed_data is None:
            return None

        return {f"S{k.split()[-1]}": v for k, v in parsed_data.items()}

    def run(self) -> bool:
        """The main execution method for the stage."""
        print("\n--- [DEBUG] Stage 1: run() method started. ---")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)
        print(f"--- [DEBUG] Stage 1: Output directory created/ensured: {self.stage_output_dir}")
        logger.info(f"Executing Stage 1: {self.stage_name} for '{self.book_stem}'")
        # TODO: Add resumability and force_book logic here later

        source_items = self._parse_source_file()
        if not source_items:
            logger.warning("      -> Source file contains no processable content.")
            print("--- [DEBUG] Stage 1: Exiting early, no source items.")
            return True

        base_lang_text = {}
        adv_target_text = {}

        # 1. Populate the 'base' tier
        print("--- [DEBUG] Stage 1: Populating base tier...")
        if self.source_lang == self.lang_config['base_code']:
            logger.info("      -> Source language matches base language. Copying text directly for base tier.")
            for item in source_items:
                if item['type'] == 'sentence':
                    base_lang_text[item['s_id']] = item['text']
            print(f"--- [DEBUG] Stage 1: Base tier populated by COPY. Got {len(base_lang_text)} items.")
        else:
            logger.info("      -> Source language does not match base language. Generating base tier via LLM.")
            base_lang_text = self._generate_text_via_llm(source_items, self.lang_config['base_code'])
            if base_lang_text is None:
                print("--- [DEBUG] Stage 1: LLM call for base tier FAILED. Exiting.")
                return False

        # 2. Populate the 'advanced_target' tier
        print("--- [DEBUG] Stage 1: Populating advanced_target tier...")
        if self.source_lang == self.lang_config['target_code']:
            logger.info("      -> Source language matches target language. Copying text directly for advanced_target tier.")
            for item in source_items:
                if item['type'] == 'sentence':
                    adv_target_text[item['s_id']] = item['text']
            print(f"--- [DEBUG] Stage 1: Target tier populated by COPY. Got {len(adv_target_text)} items.")
        else:
            logger.info("      -> Source language does not match target language. Generating advanced_target tier via LLM.")
            adv_target_text = self._generate_text_via_llm(source_items, self.lang_config['target_code'])
            if adv_target_text is None:
                print("--- [DEBUG] Stage 1: LLM call for target tier FAILED. Exiting.")
                return False
        
        book_data = {
            "book_meta": {
                "book_name": self.book_stem,
                "schema_version": "2.5-wip",
                "base_language": self.lang_config['base_code'],
                "target_language": self.lang_config['target_code']
            },
            "content_blocks": []
        }

        for item in source_items:
            if item['type'] == 'chapter':
                book_data["content_blocks"].append({
                    "block_type": "chapter_marker", "text": item["text"]
                })
            elif item['type'] == 'sentence':
                s_id = item['s_id']
                block = {
                    "block_type": "sentence",
                    "s_id": s_id,
                    "processing_status": {"stage1": "COMPLETED"},
                    "tiers": [
                        {"tier_id": "base", "full_text": base_lang_text.get(s_id, ""), "segments": []},
                        {"tier_id": "advanced_target", "full_text": adv_target_text.get(s_id, ""), "segments": []}
                    ],
                    "mappings": {}
                }
                book_data["content_blocks"].append(block)
        print(f"--- [DEBUG] Stage 1: Assembled final JSON with {len(book_data['content_blocks'])} blocks.")
        print("--- [DEBUG] Stage 1: Calling _save_output_data...")
        
        save_result = self._save_output_data(book_data, "COMPLETED")

        print(f"--- [DEBUG] Stage 1: _save_output_data returned: {save_result}")
        print("--- [DEBUG] Stage 1: run() method finished. ---")
        return save_result


        # --- THIS IS THE FIX ---
        return self._save_output_data(book_data, "COMPLETED")
