# llm2books/pool_manager.py
import json
import logging
import re
from pathlib import Path
from typing import Dict, Any, List, Optional

from . import helper, llm_prompts, llm_utils
from .llm_logger import LLMLogger
from . import standardize

logger = logging.getLogger("pipeline")

class PoolManager:
    def __init__(self, content_project_dir: Path, resources: Dict[str, Any]):
        self.content_project_root = content_project_dir
        self.resources = resources
        self.pool_dir = self.content_project_root / "common_pool"
        self.source_texts_dir = self.pool_dir / "source_texts"
        self.derived_texts_dir = self.pool_dir / "derived_texts"
        self.pool_dir.mkdir(exist_ok=True, parents=True)
        self.source_texts_dir.mkdir(exist_ok=True, parents=True)
        
        self.llm_clients = self.resources.get("llm_clients")
        self.lang_manifest = self.resources.get("language_config", {}).get("manifest", {})
        self.models_config = self.resources.get("models_config", {})
        self.pipeline_config = self.resources.get("pipeline_config", {})
        self.spacy_models = self.resources.get("spacy_models", {})
        self.stanza_processors = self.resources.get("stanza_processors", {})
        self.stages_config = self.resources.get("stages_config", {})

    #
    def _validate_segmentation_response(self, parsed_response: Dict[str, str], batch_items: List[Dict]):
        """
        A callback function to validate the content of segmentation responses.
        This is the logic that used to be in stanza_segmenter.py.
        """
        for item in batch_items:
            s_id = item['id']
            original_text = item['text']
            
            # The parsed response for segmentation is just a multi-line string.
            llm_segments_str = parsed_response.get(s_id, "")
            llm_segments = [seg.strip() for seg in llm_segments_str.splitlines() if seg.strip()]

            # The core validation: ensure no words were added or removed.
            original_words_norm = "".join(re.findall(r'[a-zA-Z0-9]+', original_text.lower()))
            llm_words_norm = "".join(re.findall(r'[a-zA-Z0-9]+', "".join(llm_segments).lower()))

            if original_words_norm != llm_words_norm:
                # Raising this error will be caught by run_llm_batch_job and trigger a retry/fallback.
                raise ValueError(f"LLM content mismatch for S_ID {s_id}. LLM modified word content.")

    def get_book_resources(self, book_stem: str, base_lang: str, target_lang: str) -> Optional[Dict[str, Path]]:
        """
        V11.1 Update: Now also generates the .mod.json simplification asset.
        """
        logger.info(f"--- PoolManager (V11.1): Gathering foundational resources for '{book_stem}' ---")
        self.book_stem = book_stem
        
        try:
            # --- START OF MODIFIED LOGIC ---
            base_std_path = self._get_or_create_std_json(base_lang)
            if not base_std_path: return None

            target_std_path = self._get_or_create_std_json(target_lang)
            if not target_std_path: return None

            # NEW: Get or create the moderate simplification of the target language.
            target_mod_path = self._get_or_create_simplified_json(target_lang, "moderate", "simplify_segments_moderate")
            if not target_mod_path: return None
            
            logger.info("--- PoolManager: Foundational resources are available. ---")
            return {
                "base_std": base_std_path,
                "target_std": target_std_path,
                "target_mod": target_mod_path, # Add the new path to the return dict
            }
            # --- END OF MODIFIED LOGIC ---
        except FileNotFoundError as e:
            logger.error(f"Halting due to critical error in PoolManager: {e}")
            return None

    # --- ADD THIS NEW METHOD TO THE PoolManager CLASS ---
    def _get_or_create_simplified_json(self, lang: str, tier_suffix: str, prompt_name: str) -> Optional[Path]:
        simplified_path = self.derived_texts_dir / f"{self.book_stem}.{lang}.{tier_suffix}.json"
        if simplified_path.exists():
            logger.info(f"  -> Found existing simplified asset: '{simplified_path.name}'")
            return simplified_path

        logger.info(f"  -> Simplified asset '{simplified_path.name}' not found. Generating...")
        
        source_std_path = self.derived_texts_dir / f"{self.book_stem}.{lang}.std.json"
        if not source_std_path.exists():
            logger.error(f"Cannot generate simplification: source '{source_std_path.name}' not found.")
            return None

        try:
            with open(source_std_path, 'r', encoding='utf-8') as f:
                source_data = json.load(f)
        except (IOError, json.JSONDecodeError) as e:
            logger.error(f"Could not read source .std.json for simplification: {e}")
            return None

        items_to_simplify = []
        for block in source_data.get("content", []):
            if block.get("block_type") == "sentence":
                for seg in block.get("segments", []):
                    items_to_simplify.append({ "id": f"{block['s_id']}_{seg['seg_id']}", "text": seg['text'] })
        
        temp_lang_config = self.resources["language_config"]
        stage_job_config = self.stages_config.get("PoolManager_Simplification", {})
        temp_path = self.derived_texts_dir / f"{self.book_stem}.{lang}.{tier_suffix}.temp.json"
        prompt = llm_prompts.get_system_prompt(prompt_name, temp_lang_config)

        simplified_segments = self._run_transactional_llm_job(
            f"Pool-Simplification-{lang}-{tier_suffix}", prompt, items_to_simplify, temp_path,
            LLMLogger(self.pool_dir / "llm_logs" / self.book_stem), "single_line", 
            stage_job_config, self.models_config, self.pipeline_config
        )

        if simplified_segments is None:
            logger.error("LLM simplification job failed to return results.")
            return None

        #
        output_content = []
        spacy_model = self.resources['spacy_models'][lang]
        for block in source_data.get("content", []):
            if block.get("block_type") == "chapter":
                output_content.append(block)
                continue
            
            if block.get("block_type") == "sentence":
                new_segments_data = []
                full_text_parts = []
                all_lemmas_for_sentence = set() # Renamed for clarity

                source_segments = block.get("segments", [])
                num_segments = len(source_segments)

                # --- START: "PARANOID MODE" REWRITE ---
                for i, original_seg in enumerate(source_segments):
                    lookup_id = f"{block['s_id']}_{original_seg['seg_id']}"
                    
                    # Get the clean simplified text, defaulting to original if missing
                    clean_simplified_text = simplified_segments.get(lookup_id, original_seg['text']).strip()
                    
                    # Determine the final text for this segment, adding a separator if needed
                    final_segment_text = clean_simplified_text
                    if i < num_segments - 1:
                        final_segment_text += " "
                    
                    full_text_parts.append(final_segment_text)

                    # Lemmatize the clean text to get the CORRECT lemmas for THIS segment
                    seg_doc = spacy_model(clean_simplified_text)
                    current_seg_lemmas = set(
                        norm_lemma for token in seg_doc if not token.is_punct and not token.is_space
                        if (norm_lemma := helper.normalize_spanish_lemma(token.lemma_))
                    )
                    
                    all_lemmas_for_sentence.update(current_seg_lemmas)
                    
                    # Build the new segment dictionary from scratch, ensuring no old data leaks.
                    new_segments_data.append({
                        "seg_id": original_seg['seg_id'],
                        "text": final_segment_text,
                        "lemmas": sorted(list(current_seg_lemmas)),
                        # Do NOT copy any other fields from original_seg
                    })
                
                output_content.append({
                    "block_type": "sentence",
                    "s_id": block['s_id'],
                    "full_text": "".join(full_text_parts).rstrip(),
                    "lemmas": sorted(list(all_lemmas_for_sentence)), # Use the newly aggregated lemmas
                    "segments": new_segments_data
                })
                # --- END: "PARANOID MODE" REWRITE ---

        final_data = {
            "meta": {"book_name": self.book_stem, "language": lang, "tier_type": tier_suffix, "schema_version": "pool-v1.0"},
            "content": output_content
        }
        
        try:
            with open(simplified_path, "w", encoding="utf-8") as f:
                json.dump(final_data, f, indent=2, ensure_ascii=False)
            if temp_path.exists(): temp_path.unlink()
            logger.info(f"  -> Successfully saved simplified asset: '{simplified_path.name}'.")
            return simplified_path
        except IOError as e:
            logger.error(f"Failed to write simplified .json file: {e}")
            return False

    def _get_or_create_std_json(self, required_lang: str) -> Optional[Path]:
        std_path = self.derived_texts_dir / f"{self.book_stem}.{required_lang}.std.json"
        if std_path.exists():
            logger.info(f"  -> Found existing foundational asset: '{std_path.name}'")
            return std_path
        
        logger.info(f"  -> Foundational asset '{std_path.name}' not found. Attempting to generate...")
        
        source_info = self._find_true_source_file()
        if not source_info:
            raise FileNotFoundError(f"Source text file for '{self.book_stem}' not found in {self.source_texts_dir}.")
        true_source_lang, _ = source_info
        
        if true_source_lang == required_lang:
            return self.generate_std_file(self.book_stem, required_lang)
        else:
            logger.info(f"    -> Dependency: '{std_path.name}' requires '{true_source_lang}' source. Checking for '{self.book_stem}.{true_source_lang}.std.json'...")
            source_std_path = self._get_or_create_std_json(true_source_lang)
            if not source_std_path:
                logger.error(f"Failed to generate dependency '{true_source_lang}.std.json' for translation.")
                return None
            
            return self._translate_and_generate_std(self.book_stem, true_source_lang, required_lang)

    def _find_true_source_file(self) -> Optional[tuple[str, Path]]:
        # ... (implementation is unchanged) ...
        glob_pattern = f"{self.book_stem}.*.txt"
        found_files = list(self.source_texts_dir.glob(glob_pattern))
        if not found_files: return None
        source_file = found_files[0]
        parts = source_file.stem.split('.')
        if len(parts) > 1: return parts[-1], source_file
        return None

    def _translate_and_generate_std(self, book_stem: str, from_lang: str, to_lang: str) -> Optional[Path]:
        # ... (implementation is unchanged) ...
        logger.info(f"    -> Translating '{book_stem}' from '{from_lang}' to '{to_lang}'...")
        source_std_path = self.derived_texts_dir / f"{book_stem}.{from_lang}.std.json"
        try:
            with open(source_std_path, 'r', encoding='utf-8') as f: source_data = json.load(f)
            source_items = source_data.get("content", [])
        except (IOError, json.JSONDecodeError) as e:
            logger.error(f"Could not read source .std.json for translation: {e}"); return None
        items_to_translate = [{"id": item['s_id'], "text": item['full_text']} for item in source_items if item.get('block_type') == 'sentence']
        
        temp_lang_config = {"base_code": from_lang, "target_code": to_lang, "manifest": self.lang_manifest, "pair_prompt_dir": self.lang_manifest.get("pair", {}).get(f"{from_lang}-{to_lang}", {}).get("prompt_dir")}
        stage_job_config = self.stages_config.get("PoolManager_Translation", {})
        temp_path = self.derived_texts_dir / f"{book_stem}.{to_lang}.translation.temp.json"
        
        lang_name_from = self.lang_manifest.get(from_lang, {}).get("name", from_lang)
        lang_name_to = self.lang_manifest.get(to_lang, {}).get("name", to_lang)
        prompt = llm_prompts.get_system_prompt("translate_text", temp_lang_config).format(source_language_name=lang_name_from, target_language_name=lang_name_to)
        config = {**self.pipeline_config, **stage_job_config}
        
        translations = self._run_transactional_llm_job(
            f"Pool-Translation-{from_lang}-to-{to_lang}", prompt, items_to_translate, temp_path,
            LLMLogger(self.pool_dir / "llm_logs" / book_stem), "single_line", config, self.models_config, self.pipeline_config
        )

        if translations is None: return None
        if len(translations) != len(items_to_translate):
            logger.error(f"Translation Integrity Check FAILED: Expected {len(items_to_translate)} translations, received {len(translations)}.")
            return None
        
        final_items = [{'type': 'sentence', 's_id': item['s_id'], 'text': translations.get(item['s_id'], "")} if item.get('block_type') == 'sentence' else {'type': 'chapter', 'text': item['text']} for item in source_items]
        
        std_file = self.generate_std_file(book_stem, to_lang, translated_items=final_items)
        if std_file and temp_path.exists(): temp_path.unlink()
        return std_file

    #
    def generate_std_file(self, book_stem: str, lang_code: str, translated_items: Optional[List[Dict]] = None) -> Optional[Path]:
        logger.info(f"  -> Generating pool file: '{book_stem}.{lang_code}.std.json'")
        std_file_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.std.json"
        
        source_items = translated_items if translated_items is not None else self._parse_source_file(self.source_texts_dir / f"{book_stem}.{lang_code}.txt")
        is_base_lang = (lang_code == self.resources['language_config']['base_code'])
        
        stanza_processor = self.resources['stanza_processors'][lang_code]
        spacy_model = self.resources['spacy_models'][lang_code]
        
        # --- START: LOGGER SWAPPING LOGIC ---
        pool_llm_logger = LLMLogger(self.pool_dir / "llm_logs" / book_stem)
        original_logger = stanza_processor.llm_logger # Save the original logger
        stanza_processor.llm_logger = pool_llm_logger # Assign the new, correct logger
        # --- END: LOGGER SWAPPING LOGIC ---
        
        segmentation_results = {}

        try: # Use a try...finally block to ensure the original logger is always restored
            if not is_base_lang:
                temp_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.segmentation.temp.json"
                if temp_path.exists():
                    try:
                        with open(temp_path, 'r', encoding='utf-8') as f:
                            segmentation_results = json.load(f)
                        logger.info(f"      -> Resuming segmentation from progress file with {len(segmentation_results)} completed items.")
                    except (IOError, json.JSONDecodeError):
                        logger.warning("      -> Could not read segmentation progress file. Starting fresh.")

                items_to_segment = [item for item in source_items if item.get('type') == 'sentence' and item.get('s_id') not in segmentation_results]

                if items_to_segment:
                    logger.info(f"      -> Preparing to segment {len(items_to_segment)} new sentences...")
                    
                    for i, item in enumerate(items_to_segment):
                        s_id, original_text = item['s_id'], item['text']
                        logger.info(f"        -> Processing item {i+1}/{len(items_to_segment)}: {s_id}")

                        if not original_text.strip():
                            segmentation_results[s_id] = []
                            continue
                        
                        try:
                            # Now this call will use the correct pool_llm_logger
                            segments_text_list = stanza_processor.segment_sentence(original_text, s_id)
                            segmentation_results[s_id] = segments_text_list
                            
                            with open(temp_path, 'w', encoding='utf-8') as f:
                                json.dump(segmentation_results, f, indent=2)

                        except (IOError, ValueError) as e:
                            logger.error(f"      -> FAILED to segment S_ID {s_id}. Reason: {e}")
                            logger.error("         The pipeline has been halted. Your progress is saved.")
                            return None
            
            # --- ASSEMBLY LOGIC (inside the try block) ---
            logger.info("      -> Assembling final .std.json from all segmentation results...")
            output_content = []
            for item in source_items:
                # ... (the entire assembly loop from before goes here, unchanged) ...
                if item['type'] == 'chapter':
                    output_content.append({"block_type": "chapter", "text": item['text']})
                    continue
                if item['type'] == 'sentence':
                    s_id, original_text = item['s_id'], item['text']
                    full_text = helper.preprocess_for_spacy(original_text)
                    if is_base_lang:
                        segments_text = [full_text]
                    else:
                        segments_text = segmentation_results.get(s_id, [])
                    spacy_doc = spacy_model(full_text)
                    golden_stream = helper.create_golden_token_stream(spacy_doc)
                    word_tokens = [tok for tok in golden_stream if tok['t'] == 'w']
                    current_word_idx = 0
                    for seg_idx, seg_str in enumerate(segments_text):
                        num_words = len(re.findall(r'\w+', seg_str))
                        for _ in range(num_words):
                            if current_word_idx < len(word_tokens): word_tokens[current_word_idx]['seg_idx'] = seg_idx
                            current_word_idx += 1
                    num_segments = len(segments_text)
                    if num_segments == 0 and not full_text.strip(): continue
                    if num_segments == 0 and full_text.strip(): num_segments = 1; segments_text = [full_text]
                    buckets: List[List[Dict]] = [[] for _ in range(num_segments)]
                    b_idx = 0
                    for token in golden_stream:
                        seg_idx = token.get('seg_idx', b_idx)
                        if seg_idx < num_segments:
                            buckets[seg_idx].append(token)
                            if token['t'] == 'w': b_idx = seg_idx
                    for i in range(num_segments - 1):
                        if buckets[i] and buckets[i+1]:
                            if buckets[i][-1]['t'] == 'w': buckets[i].append({'t':'b', 'v':''})
                            if buckets[i+1][0]['t'] == 'w': buckets[i+1].insert(0, {'t':'b', 'v':''})
                            b1, b2 = buckets[i][-1], buckets[i+1][0]
                            combined = b1['v'] + b2['v']
                            split_idx = combined.find(' ')
                            if split_idx != -1: b1['v'], b2['v'] = combined[:split_idx + 1], combined[split_idx + 1:]
                            else: b1['v'], b2['v'] = combined, ""
                    segments_data, all_lemmas, di_counter = [], set(), 0
                    for i, bucket in enumerate(buckets):
                        seg_text = "".join(tok['v'] for tok in bucket)
                        seg_doc = spacy_model(seg_text)
                        seg_lemmas = set()
                        for token in bucket:
                            if token.get('t') == 'w':
                                token['di'] = di_counter; di_counter += 1
                                for st in seg_doc:
                                    if st.text == token['v'] and st.idx == seg_text.find(token['v']):
                                        lemma = helper.normalize_spanish_lemma(st.lemma_) if lang_code == 'es' else st.lemma_.lower().strip()
                                        if lemma: token['l'] = [lemma]; all_lemmas.add(lemma); seg_lemmas.add(lemma)
                                        break
                        segments_data.append({ "seg_id": f"S{i+1}", "text": seg_text, "tokenized_text": bucket, "lemmas": sorted(list(seg_lemmas))})
                    output_content.append({ "block_type": "sentence", "s_id": s_id, "full_text": full_text, "lemmas": sorted(list(all_lemmas)), "segments": segments_data })

            final_data = { "meta": { "book_name": book_stem, "language": lang_code, "tier_type": "std", "schema_version": "pool-v1.0" }, "content": output_content }
            
            try:
                with open(std_file_path, "w", encoding="utf-8") as f: json.dump(final_data, f, indent=2, ensure_ascii=False)
                logger.info(f"  -> Successfully saved '{std_file_path.name}'.")
                temp_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.segmentation.temp.json"
                if temp_path.exists(): temp_path.unlink()
                return std_file_path
            except IOError as e:
                logger.error(f"Failed to write .std.json file: {e}"); return None

        finally:
            # This will execute whether the try block succeeds or fails, ensuring the logger is always restored.
            stanza_processor.llm_logger = original_logger

    #
    def _run_transactional_llm_job(self, job_name, system_prompt, all_items, temp_progress_path, llm_logger, parser_type, stage_config, models_config, pipeline_config):
        """
        Runs a batch LLM job with transactional saving to a temporary file.
        """
        completed = {}
        if temp_progress_path.exists():
            try:
                with open(temp_progress_path, 'r', encoding='utf-8') as f:
                    completed = json.load(f)
                logger.info(f"      -> Resuming job '{job_name}' from progress file with {len(completed)} completed items.")
            except (IOError, json.JSONDecodeError):
                logger.warning(f"      -> Could not read progress file for '{job_name}'. Starting fresh.")

        items_to_run = [i for i in all_items if i['id'] not in completed]
        if not items_to_run:
            logger.info(f"      -> Job '{job_name}' is already complete.")
            return completed

        batch_size = stage_config.get("batch_size_in_items", 10)
        
        for i in range(0, len(items_to_run), batch_size):
            batch = items_to_run[i:i + batch_size]
            
            # This is the key change: it calls the central utility function
            results = llm_utils.run_llm_batch_job(
                self.llm_clients, job_name, system_prompt, batch, llm_logger, parser_type, 
                stage_config, models_config, pipeline_config
            )

            if not results:
                logger.error(f"      -> A batch failed for job '{job_name}'. Halting.")
                return None # Signal failure
            
            for item in results:
                completed[item['id']] = item['llm_response']
            
            # Save progress after every successful batch
            try:
                with open(temp_progress_path, 'w', encoding='utf-8') as f:
                    json.dump(completed, f, indent=2)
            except IOError as e:
                logger.error(f"      -> CRITICAL: Could not write progress for '{job_name}'. Error: {e}")
                return None # Signal critical failure

        return completed

    def _parse_source_file(self, file_path: Path) -> List[Dict[str, Any]]:
        text = file_path.read_text(encoding="utf-8")
        lines = text.splitlines()
        start = 1 if lines and lines[0].startswith("%%lang:") else 0
        
        # Correct regex captures the S-ID and the text separately
        s_re = re.compile(r"^{(S\d+):\s*(.*)}$")
        c_re = re.compile(r"^%%CHAPTER_MARKER%%\s*(.*)$")
        
        items = []
        for line in (l.strip() for l in lines[start:] if l.strip()):
            if (m := c_re.match(line)):
                items.append({"type": "chapter", "text": m.group(1).strip()})
            elif (m := s_re.match(line)):
                # m.group(1) is "S1", m.group(2) is the sentence text. No int conversion needed.
                items.append({"type": "sentence", "s_id": m.group(1), "text": m.group(2).strip()})
        return items