# llm2books/pool_manager.py

import json
import logging
import re
from pathlib import Path
from typing import Dict, Any, List, Optional
import time

from . import helper
from . import llm_prompts
from .stanza_segmenter import StanzaLanguageProcessor
from .llm_logger import LLMLogger

logger = logging.getLogger("pipeline")

class PoolManager:
    def __init__(self, content_project_dir: Path, resources: Dict[str, Any]):
        self.content_project_root = content_project_dir
        self.resources = resources
        self.pool_dir = self.content_project_root / "common_pool"
        self.source_texts_dir = self.pool_dir / "source_texts"
        self.derived_texts_dir = self.pool_dir / "derived_texts"
        self.pool_dir.mkdir(exist_ok=True)
        self.source_texts_dir.mkdir(exist_ok=True)
        self.derived_texts_dir.mkdir(exist_ok=True)
        self.spacy_models = self.resources.get("spacy_models", {})
        self.stanza_processors = self.resources.get("stanza_processors", {})
        self.llm_client = self.resources.get("llm_client")
        lang_config = self.resources.get("language_config", {})
        self.lang_manifest = lang_config.get("manifest", {}) if lang_config else {}

    def _parse_singleline_llm_response(self, raw_text: str) -> Dict[str, str]:
        parsed = {}
        # This more general regex captures:
        # (^[^:]+) - A capturing group for the ID (any characters except a colon at the start)
        # \s*:\s*  - The colon separator
        # (.*)     - The rest of the line
        line_regex = re.compile(r"^([^:]+):\s*(.*)$")
        
        for line in raw_text.splitlines():
            match = line_regex.match(line)
            if match:
                # We use .strip() to be safe
                pid = match.group(1).strip()
                text = match.group(2).strip()
                parsed[pid] = text
        return parsed

    def _parse_multiline_llm_response(self, raw_text: str) -> Dict[str, str]:
        parsed = {}
        current_id = None
        current_lines = []
        
        lines_to_parse = raw_text.strip().splitlines() + [""]
        
        for line in lines_to_parse:
            # --- THIS IS THE FIX ---
            # Use a more general regex that captures the ID and strips it.
            # This regex is identical to the one in the single-line parser.
            # It handles S1_S1, S1_A1, etc.
            match = re.match(r"^([^:]+):(.*)$", line)
            # --- END OF FIX ---

            if match:
                # If we were processing a previous ID, save its collected lines
                if current_id:
                    parsed[current_id] = "\n".join(current_lines).strip()
                
                # Start processing the new ID
                # --- THIS IS THE FIX ---
                current_id = match.group(1).strip()
                # Also strip the initial text after the colon
                current_lines = [match.group(2).strip()]
                # --- END OF FIX ---
            elif current_id:
                # If we are inside a block, just append the line
                current_lines.append(line.strip())
        
        # Ensure the final block is added
        if current_id and current_id not in parsed:
             parsed[current_id] = "\n".join(current_lines).strip()
             
        return parsed

    def get_book_resources(self, book_stem: str, base_lang: str, target_lang: str) -> Optional[Dict[str, Path]]:
        logger.info(f"--- PoolManager: Gathering resources for '{book_stem}' ({base_lang} -> {target_lang}) ---")
        base_std_path = self.derived_texts_dir / f"{book_stem}.{base_lang}.std.json"
        target_std_path = self.derived_texts_dir / f"{book_stem}.{target_lang}.std.json"
        target_sim_path = self.derived_texts_dir / f"{book_stem}.{target_lang}.sim.json"

        if not base_std_path.exists():
            logger.info(f"Base std file '{base_std_path.name}' not found. Generating...")
            if not self.generate_std_file(book_stem, base_lang): return None

        if not target_std_path.exists():
            logger.info(f"Target std file '{target_std_path.name}' not found. Generating via translation...")
            if not self._translate_and_generate_std(book_stem, base_lang, target_lang): return None

        if not target_sim_path.exists():
            logger.info(f"Target sim file '{target_sim_path.name}' not found. Generating...")
            if not self.generate_sim_file(book_stem, target_lang): return None
        
        logger.info("--- PoolManager: All required resources are available. ---")
        return {"base_std": base_std_path, "target_std": target_std_path, "target_sim": target_sim_path}

    def generate_std_file(self, book_stem: str, lang_code: str, translated_items: Optional[List[Dict]] = None) -> Optional[Path]:
        logger.info(f"  -> Generating pool file: '{book_stem}.{lang_code}.std.json'")
        std_file_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.std.json"
        
        if translated_items:
            source_items = translated_items
        else:
            source_path = self.source_texts_dir / f"{book_stem}.{lang_code}.txt"
            if not source_path.exists():
                logger.error(f"PoolManager: Source text not found at {source_path}."); return None
            try:
                source_items = self._parse_source_file(source_path)
            except Exception as e:
                logger.error(f"Failed to parse source file {source_path.name}: {e}"); return None
        
        stanza_processor: Optional[StanzaLanguageProcessor] = self.stanza_processors.get(lang_code)
        spacy_model = self.spacy_models.get(lang_code)
        if not stanza_processor or not spacy_model:
            logger.error(f"Missing Stanza or SpaCy processor for language '{lang_code}'."); return None
            
        output_content = []
        for item in source_items:
            if item['type'] == 'chapter': continue
            s_id, full_text = item['s_id'], item['text']
            segment_texts = stanza_processor.segment_sentence(full_text)
            segments_data = []
            sentence_di_counter = 0
            full_doc = spacy_model(full_text)
            token_to_lemma = { token.text: helper.normalize_spanish_lemma(token.lemma_) for token in full_doc if not token.is_punct and not token.is_space }
            all_sentence_lemmas = set(token_to_lemma.values())
            for i, seg_text in enumerate(segment_texts):
                seg_doc = spacy_model(seg_text)
                token_list = helper.create_v2_token_list(seg_doc[:])
                for token in token_list:
                    if token.get("t") == "w": token["di"] = sentence_di_counter; sentence_di_counter += 1
                segments_data.append({ "seg_id": f"S{i+1}", "tokenized_text": token_list })
            output_content.append({ "s_id": s_id, "full_text": full_text, "lemmas": sorted(list(l for l in all_sentence_lemmas if l)), "segments": segments_data })
        
        final_json_data = { "meta": { "book_name": book_stem, "language": lang_code, "tier_type": "std", "schema_version": "pool-v1.0" }, "content": output_content }
        try:
            with open(std_file_path, "w", encoding="utf-8") as f: json.dump(final_json_data, f, indent=2, ensure_ascii=False)
            logger.info(f"  -> Successfully saved '{std_file_path.name}' to common pool.")
            return std_file_path
        except IOError as e:
            logger.error(f"Failed to write .std.json file to {std_file_path}: {e}"); return None

    def _translate_and_generate_std(self, book_stem: str, from_lang: str, to_lang: str) -> Optional[Path]:
        logger.info(f"    -> Translating '{book_stem}' from '{from_lang}' to '{to_lang}'...")
        source_path = self.source_texts_dir / f"{book_stem}.{from_lang}.txt"
        if not source_path.exists():
            logger.error(f"Cannot translate: Source file '{source_path.name}' not found."); return None
        
        source_items = self._parse_source_file(source_path)
        items_to_translate = [item for item in source_items if item['type'] == 'sentence']

        from_lang_name = self.lang_manifest.get(from_lang, {}).get("name", from_lang)
        to_lang_name = self.lang_manifest.get(to_lang, {}).get("name", to_lang)
        
        system_prompt_template = llm_prompts.get_system_prompt(
            "translate_text", 
            self.resources["language_config"]
        )
        system_prompt = system_prompt_template.format(source_language_name=from_lang_name, target_language_name=to_lang_name)

        llm_logger = LLMLogger(self.pool_dir / "llm_logs" / book_stem)
        translation_results = self._run_llm_batch_job(
            job_name=f"Translation-{from_lang}-to-{to_lang}",
            system_prompt=system_prompt,
            items_to_process=items_to_translate,
            id_prefix=None,
            llm_logger=llm_logger # Pass the logger
        )

        if not translation_results:
            logger.error("LLM translation failed."); return None

        translated_items = []
        result_map = {item['s_id']: item['llm_response'] for item in translation_results}
        for item in source_items:
            if item['type'] == 'sentence':
                item['text'] = result_map.get(item['s_id'], item['text'])
            translated_items.append(item)
            
        return self.generate_std_file(book_stem, to_lang, translated_items=translated_items)

    #
    def generate_sim_file(self, book_stem: str, lang_code: str) -> Optional[Path]:
        """
        Generates a simpler derived tier file (e.g., 'Book.es.sim.json')
        with dynamic diagnostics to trace data persistence.
        """
        logger.info(f"  -> Generating pool file: '{book_stem}.{lang_code}.sim.json'")
        sim_file_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.sim.json"
        std_target_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.std.json"
        base_lang_code = self.resources['language_config']['base_code']
        std_base_path = self.derived_texts_dir / f"{book_stem}.{base_lang_code}.std.json"
        if not std_target_path.exists() or not std_base_path.exists():
            logger.error(f"Cannot generate .sim file: Required parent .std file(s) not found."); return None
        try:
            with open(std_target_path, 'r', encoding='utf-8') as f: std_target_data = json.load(f)
            with open(std_base_path, 'r', encoding='utf-8') as f: std_base_data = json.load(f)
        except Exception as e:
            logger.error(f"Failed to read or parse parent .std files: {e}"); return None
        llm_logger = LLMLogger(self.pool_dir / "llm_logs" / book_stem)
        target_sentences = std_target_data.get("content", [])
        
        # LLM Call 1: Simplify
        simplify_prompt = llm_prompts.get_system_prompt("simplify_segments", self.resources["language_config"])
        items_to_simplify = [{"s_id": s['s_id'], "seg_id": seg['seg_id'], "text": "".join(t['v'] for t in seg['tokenized_text']).strip()} for s in target_sentences for seg in s.get("segments", []) if "".join(t['v'] for t in seg['tokenized_text']).strip()]
        simplified_results = self._run_llm_batch_job(job_name="Simplification", system_prompt=simplify_prompt, items_to_process=items_to_simplify, llm_logger=llm_logger)
        if simplified_results is None: return None
        simplified_results_map = {f"{item['s_id']}_{item['seg_id']}": item['llm_response'] for item in simplified_results}

        # LLM Call 2: Inverse Diglot
        inv_diglot_prompt = llm_prompts.get_system_prompt("generate_inverse_diglot", self.resources["language_config"])
        items_for_inv_diglot = [{"s_id": item['s_id'], "seg_id": item['seg_id'], "text": item['llm_response']} for item in simplified_results if item['llm_response'].strip()]
        inv_diglot_results = self._run_llm_batch_job(job_name="InverseDiglot", system_prompt=inv_diglot_prompt, items_to_process=items_for_inv_diglot, llm_logger=llm_logger)
        if inv_diglot_results is None: return None
        
        spacy_model = self.spacy_models.get(lang_code)
        if not spacy_model: logger.error(f"Missing SpaCy processor for '{lang_code}'."); return None

        # --- EXPLICIT LOOP FOR PARSING ---
        inverse_diglot_maps = {}
        map_regex = re.compile(r"^\s*([^->]+?)\s*->\s*(.+)$")
        for item in inv_diglot_results:
            # Use distinct variable names to avoid any possible scope collision
            item_s_id, item_seg_id = item['s_id'], item['seg_id']
            if item_s_id not in inverse_diglot_maps:
                inverse_diglot_maps[item_s_id] = {}
            
            mappings = []
            for line in item['llm_response'].splitlines():
                match = map_regex.match(line)
                if match:
                    target_word, base_sub = match.groups()
                    mappings.append({"target_word": target_word.strip(), "base_substitute": base_sub.strip()})
            inverse_diglot_maps[item_s_id][item_seg_id] = mappings
        # --- END OF EXPLICIT LOOP ---

        output_content = []
        for sentence_data in target_sentences:
            s_id = sentence_data['s_id']
            
            # --- EXPLICIT LOOP for simpler_segment_texts ---
            simpler_segment_texts = []
            for seg in sentence_data.get("segments", []):
                lookup_key = f"{s_id}_{seg['seg_id']}"
                original_text = "".join(t['v'] for t in seg['tokenized_text'])
                text = simplified_results_map.get(lookup_key, original_text)
                simpler_segment_texts.append(text)
            # --- END OF EXPLICIT LOOP ---

            simpler_full_text = " ".join(simpler_segment_texts)
            full_doc = spacy_model(simpler_full_text)
            token_to_lemma = {token.text: helper.normalize_spanish_lemma(token.lemma_) for token in full_doc if not token.is_punct and not token.is_space}
            all_sentence_lemmas = set(token_to_lemma.values())
            segments_data = []
            for i, seg_text in enumerate(simpler_segment_texts):
                seg_doc = spacy_model(seg_text)
                token_list = helper.create_v2_token_list(seg_doc[:])
                seg_lemmas = {token_to_lemma.get(t['v']) for t in token_list if t['t'] == 'w' and token_to_lemma.get(t['v'])}
                segments_data.append({"seg_id": sentence_data['segments'][i]['seg_id'], "tokenized_text": token_list, "lemmas_per_segment": sorted(list(seg_lemmas))})
            
            sentence_inverse_diglot_map = inverse_diglot_maps.get(s_id, {})
            
            output_content.append({"s_id": s_id, "full_text": simpler_full_text, "lemmas": sorted(list(l for l in all_sentence_lemmas if l)), "segments": segments_data, "inverse_diglot_map": sentence_inverse_diglot_map})
        
        final_json_data = {"meta": {"book_name": book_stem, "language": lang_code, "tier_type": "sim", "schema_version": "pool-v1.0", "parent_std_file": std_target_path.name}, "content": output_content}
        try:
            with open(sim_file_path, "w", encoding="utf-8") as f: json.dump(final_json_data, f, indent=2, ensure_ascii=False)
            logger.info(f"  -> Successfully saved '{sim_file_path.name}' to common pool.")
            return sim_file_path
        except IOError as e:
            logger.error(f"Failed to write .sim.json file to {sim_file_path}: {e}"); return None

    def _run_llm_batch_job(self, job_name: str, system_prompt: str, items_to_process: List[Dict], llm_logger: LLMLogger, id_prefix: Optional[str] = "") -> Optional[List[Dict]]:
        BATCH_SIZE, MAX_RETRIES, RETRY_DELAY = 10, 3, 5
        all_results = []
        batch_num = 0
        for i in range(0, len(items_to_process), BATCH_SIZE):
            batch_num += 1
            batch = items_to_process[i:i + BATCH_SIZE]
            
            prompt_ids = []
            if id_prefix is not None:
                prompt_ids = [f"{item['s_id']}_{item['seg_id']}" for item in batch]
            else:
                prompt_ids = [item['s_id'] for item in batch]

            user_prompt = "\n".join([f"{pid}: {item['text']}" for pid, item in zip(prompt_ids, batch)])
            logger.info(f"    -> Running {job_name} LLM batch {batch_num}...")
            
            for _ in range(MAX_RETRIES):
                raw_response = ""
                try:
                    message = self.llm_client.messages.create(model="claude-3-haiku-20240307", system=system_prompt, messages=[{"role": "user", "content": user_prompt}], max_tokens=4096)
                    raw_response = message.content[0].text

                    if job_name == "Simplification":
                        parsed_response = self._parse_singleline_llm_response(raw_response)
                    else:
                        parsed_response = self._parse_multiline_llm_response(raw_response)
                    expected_ids_normalized = {pid.strip() for pid in prompt_ids}
                    parsed_ids_normalized = set(parsed_response.keys()) # Keys are already stripped by the parser
                    if expected_ids_normalized.issubset(parsed_ids_normalized):
                    # --- END OF FIX ---
                        for item, pid in zip(batch, prompt_ids):
                            # We must still use the original pid to look up in the original item list
                            # But we use the stripped version for the parsed_response
                            item['llm_response'] = parsed_response[pid.strip()]
                        all_results.extend(batch)
                        break
                    else:
                        missing_ids = expected_ids_normalized - parsed_ids_normalized
                        logger.warning(f"      -> {job_name} batch failed validation (missing IDs: {missing_ids}). Retrying...")

                    llm_logger.log_batch(job_name, batch_num, system_prompt, user_prompt, raw_response)
                    ##
                    parsed_response = {}
                    current_id = None
                    current_lines = []

                    lines_to_parse = raw_response.strip().splitlines() + [""]
                    
                    for line in lines_to_parse:
                        # Check if the line looks like a new ID
                        match = re.match(r"^(S\d+(_[AS]\d+)?):", line.strip())
                        if match:
                            # If we were processing a previous ID, save its collected lines
                            if current_id:
                                parsed_response[current_id] = "\n".join(current_lines).strip()
                            
                            # Start processing the new ID
                            current_id = match.group(1)
                            current_lines = [line.split(":", 1)[1].strip()] # Get text after colon
                        elif current_id:
                            # If we are inside a block, just append the line
                            current_lines.append(line.strip())
                    ##
                    if all(pid in parsed_response for pid in prompt_ids):
                        for item, pid in zip(batch, prompt_ids):
                            item['llm_response'] = parsed_response[pid]
                        all_results.extend(batch)
                        break
                    else:
                        missing_ids = [pid for pid in prompt_ids if pid not in parsed_response]
                        logger.warning(f"      -> {job_name} batch failed validation (missing IDs: {missing_ids}). Retrying...")

                except Exception as e:
                    logger.error(f"      -> API Error during {job_name} batch: {e}. Retrying...")
                    # Log the failed response if we have it
                    if raw_response:
                        llm_logger.log_batch(job_name, batch_num, system_prompt, user_prompt, f"FAILED_RESPONSE: {raw_response}\nERROR: {e}")

                time.sleep(RETRY_DELAY)
            else:
                logger.error(f"      -> {job_name} batch failed after {MAX_RETRIES} retries. Aborting job.")
                return None
        return all_results

    def _parse_source_file(self, file_path: Path) -> List[Dict[str, Any]]:
        text = file_path.read_text(encoding="utf-8")
        lines = text.splitlines()[1:]
        all_items, sentence_regex, chapter_regex = [], re.compile(r"^{S(\d+):\s*(.*)}$"), re.compile(r"^%%CHAPTER_MARKER%%\s*(.*)$")
        for line in (l.strip() for l in lines if l.strip()):
            if (m := chapter_regex.match(line)):
                all_items.append({"type": "chapter", "text": m.group(1).strip()})
            elif (m := sentence_regex.match(line)):
                all_items.append({"type": "sentence", "s_id": f"S{int(m.group(1))}", "text": m.group(2).strip()})
        return all_items