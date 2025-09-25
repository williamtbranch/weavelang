import json
import logging
import re
from pathlib import Path
from typing import Dict, Any, List, Optional

from . import helper, llm_prompts, validator, llm_utils
from .llm_logger import LLMLogger

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
        self.llm_client = self.resources.get("llm_client")
        self.lang_manifest = self.resources.get("language_config", {}).get("manifest", {})
        self.models_config = self.resources.get("models_config", {})
        self.pipeline_config = self.resources.get("pipeline_config", {})
        self.spacy_models = self.resources.get("spacy_models", {})
        self.stanza_processors = self.resources.get("stanza_processors", {})
        self.stages_config = self.resources.get("stages_config", {})

    def _batch_segments_by_sentence(
        self,
        sentences: List[Dict[str, Any]],
        stage_config: Dict[str, Any]
    ) -> List[List[Dict[str, str]]]:
        all_batches = []
        current_batch = []
        batch_size_in_items = stage_config.get("batch_size_in_items", 50) 

        for sentence in sentences:
            s_id = sentence['s_id']
            segments_for_sentence = [
                {
                    "id": f"{s_id}_{seg['seg_id']}",
                    "text": " ".join(seg.get("text", "").strip().split())
                }
                for seg in sentence.get("segments", []) if seg.get("text", "").strip()
            ]

            if not segments_for_sentence:
                continue

            if current_batch and (len(current_batch) + len(segments_for_sentence) > batch_size_in_items):
                all_batches.append(current_batch)
                current_batch = []

            current_batch.extend(segments_for_sentence)

        if current_batch:
            all_batches.append(current_batch)
            
        logger.info(f"      -> Grouped {sum(len(b) for b in all_batches)} segments into {len(all_batches)} sentence-aware batches.")
        return all_batches

    def get_book_resources(self, book_stem: str, base_lang: str, target_lang: str) -> Optional[Dict[str, Path]]:
        logger.info(f"--- PoolManager: Gathering resources for '{book_stem}' ({base_lang} -> {target_lang}) ---")
        self.book_stem = book_stem
        
        try:
            base_std_path = self._get_or_create_std_json(base_lang)
            if not base_std_path: return None

            target_std_path = self._get_or_create_std_json(target_lang)
            if not target_std_path: return None

            target_mod_path = self._get_or_create_mod_json(target_lang)
            if not target_mod_path: return None
            
            target_bas_path = self._get_or_create_bas_json(target_lang)
            if not target_bas_path: return None

            target_sim_path = self._get_or_create_sim_json(target_lang)
            if not target_sim_path: return None
            
            logger.info("--- PoolManager: All required resources are now available. ---")
            return {
                "base_std": base_std_path,
                "target_std": target_std_path,
                "target_mod": target_mod_path,
                "target_bas": target_bas_path,
                "target_sim": target_sim_path,
            }
        except FileNotFoundError as e:
            logger.error(f"Halting due to critical error: {e}")
            return None

    def _get_or_create_mod_json(self, target_lang: str) -> Optional[Path]:
        mod_path = self.derived_texts_dir / f"{self.book_stem}.{target_lang}.mod.json"
        if mod_path.exists():
            logger.info(f"  -> Found existing asset: '{mod_path.name}'")
            return mod_path

        logger.info(f"  -> Asset '{mod_path.name}' not found. Attempting to generate...")
        
        logger.info(f"    -> Dependency: '{mod_path.name}' requires '{self.book_stem}.{target_lang}.std.json'.")
        target_std_path = self._get_or_create_std_json(target_lang)
        if not target_std_path:
            logger.error(f"Failed to generate dependency '{target_lang}.std.json' for moderate simplification.")
            return None
            
        return self._generate_derived_file(
            book_stem=self.book_stem,
            lang_code=target_lang,
            tier_suffix="mod",
            prompt_name="simplify_segments_moderate",
            job_name_suffix="Moderate"
        )

    def _get_or_create_bas_json(self, target_lang: str) -> Optional[Path]:
        bas_path = self.derived_texts_dir / f"{self.book_stem}.{target_lang}.bas.json"
        if bas_path.exists():
            logger.info(f"  -> Found existing asset: '{bas_path.name}'")
            return bas_path

        logger.info(f"  -> Asset '{bas_path.name}' not found. Attempting to generate...")
        
        logger.info(f"    -> Dependency: '{bas_path.name}' requires '{self.book_stem}.{target_lang}.std.json'.")
        target_std_path = self._get_or_create_std_json(target_lang)
        if not target_std_path:
            logger.error(f"Failed to generate dependency '{target_lang}.std.json' for basic simplification.")
            return None
            
        return self._generate_derived_file(
            book_stem=self.book_stem,
            lang_code=target_lang,
            tier_suffix="bas",
            prompt_name="simplify_segments_basic",
            job_name_suffix="Basic"
        )
        
    def _get_or_create_sim_json(self, target_lang: str) -> Optional[Path]:
        sim_path = self.derived_texts_dir / f"{self.book_stem}.{target_lang}.sim.json"
        if sim_path.exists():
            logger.info(f"  -> Found existing asset: '{sim_path.name}'")
            return sim_path
        
        logger.info(f"  -> Asset '{sim_path.name}' not found. Attempting to generate...")
        
        logger.info(f"    -> Dependency: '{sim_path.name}' requires '{self.book_stem}.{target_lang}.std.json'.")
        target_std_path = self._get_or_create_std_json(target_lang)
        if not target_std_path:
            logger.error(f"Failed to generate dependency '{target_lang}.std.json' for simple simplification.")
            return None
            
        return self._generate_derived_file(
            book_stem=self.book_stem,
            lang_code=target_lang,
            tier_suffix="sim",
            prompt_name="simplify_segments_simple",
            job_name_suffix="Simple"
        )
        
    def _load_manual_overrides(self, job_name: str) -> Dict[str, str]:
        """Scans an LLM log file for a %%MANUAL_FIX%% block and parses it."""
        override_map = {}
        log_file = self.pool_dir / "llm_logs" / self.book_stem / f"{job_name}.log"
        
        if not log_file.exists():
            return override_map

        try:
            content = log_file.read_text(encoding="utf-8")
            if "%%MANUAL_FIX%%" not in content:
                return override_map

            logger.warning(f"  -> Found %%MANUAL_FIX%% block in '{log_file.name}'. Applying overrides.")
            
            # Extract the content after the last occurrence of the keyword
            fix_block = content.split("%%MANUAL_FIX%%")[-1]
            
            for line in fix_block.splitlines():
                if ":" in line:
                    parts = line.split(":", 1)
                    if len(parts) == 2:
                        item_id = parts[0].strip()
                        fixed_text = parts[1].strip()
                        if item_id and fixed_text:
                            override_map[item_id] = fixed_text
                            logger.info(f"     -> Loaded manual fix for ID: {item_id}")
            
        except Exception as e:
            logger.error(f"Could not parse manual override block from {log_file.name}: {e}")

        return override_map

    def _generate_derived_file(self, book_stem: str, lang_code: str, tier_suffix: str, prompt_name: str, job_name_suffix: str) -> Optional[Path]:
        derived_file_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.{tier_suffix}.json"
        std_target_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.std.json"
        job_name = f"Pool-Simplification-{job_name_suffix}"
        
        with open(std_target_path, 'r', encoding='utf-8') as f:
            std_data = json.load(f)

        sentences = [b for b in std_data.get("content", []) if b.get("block_type") == "sentence"]
        if not sentences:
            logger.warning(f"Source has no sentences. Creating empty {tier_suffix} file.")
            with open(derived_file_path, "w", encoding="utf-8") as f:
                json.dump({**std_data, "content": [b for b in std_data.get("content", []) if b.get("block_type") != "sentence"]}, f, indent=2, ensure_ascii=False)
            return derived_file_path

        temp_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.{tier_suffix}.temp.json"
        from_lang = self.resources.get("language_config", {}).get("base_code", "en")
        temp_lang_config = {
            "base_code": from_lang, "target_code": lang_code, "manifest": self.lang_manifest,
            "pair_prompt_dir": self.lang_manifest.get("pair", {}).get(f"{from_lang}-{lang_code}", {}).get("prompt_dir")
        }
        prompt = llm_prompts.get_system_prompt(prompt_name, temp_lang_config)
        stage_job_config = self.stages_config.get("PoolManager_Simplification", {})
        config = {**self.pipeline_config, **stage_job_config}
        llm_logger = LLMLogger(self.pool_dir / "llm_logs" / book_stem)
        
        # --- NEW: MANUAL OVERRIDE LOGIC ---
        manual_overrides = self._load_manual_overrides(job_name)

        segment_batches = self._batch_segments_by_sentence(sentences, config)
        
        completed_results = {}
        if temp_path.exists():
            try:
                with open(temp_path, 'r', encoding='utf-8') as f:
                    completed_results = json.load(f)
            except (IOError, json.JSONDecodeError):
                completed_results = {}

        for i, batch in enumerate(segment_batches):
            batch_ids = {item['id'] for item in batch}
            if batch_ids.issubset(completed_results.keys()):
                logger.info(f"      -> Skipping batch {i+1}/{len(segment_batches)} for {tier_suffix}, all segments already processed.")
                continue

            items_to_run_in_llm = []
            for item in batch:
                if item['id'] in manual_overrides:
                    # Apply the manual override and treat it as a completed result
                    logger.info(f"      -> Applying manual override for {item['id']} in batch {i+1}.")
                    completed_results[item['id']] = manual_overrides[item['id']]
                elif item['id'] not in completed_results:
                    items_to_run_in_llm.append(item)
            
            if not items_to_run_in_llm:
                logger.info(f"      -> All items in batch {i+1} were either cached or manually fixed. Saving progress.")
            else:
                logger.info(f"      -> Processing batch {i+1}/{len(segment_batches)} for {tier_suffix} ({len(items_to_run_in_llm)} new segments)...")
                batch_results_list = llm_utils.run_llm_batch_job(
                    self.llm_client, job_name, prompt, items_to_run_in_llm,
                    llm_logger, "single_line", config, self.models_config
                )
                if batch_results_list is None: return None
                for item in batch_results_list:
                    completed_results[item['id']] = item['llm_response']
            
            with open(temp_path, 'w', encoding='utf-8') as f:
                json.dump(completed_results, f, indent=2)
        
        results = completed_results
        spacy = self.resources['spacy_models'][lang_code]
        output_content = [b for b in std_data.get("content", []) if b.get("block_type") != "sentence"]

        for s_block in sentences:
            s_id = s_block['s_id']
            new_segs, new_text_parts = [], []
            for seg in s_block["segments"]:
                original_seg_text = seg["text"]
                lookup_key = f"{s_id}_{seg['seg_id']}"
                
                # --- THIS IS THE GRACEFUL FALLBACK ---
                # Use the result if available, otherwise fall back to the original text.
                simplified_text = results.get(lookup_key, original_seg_text).strip()
                if not simplified_text: # If the result was an empty string, also fall back
                    simplified_text = original_seg_text

                if original_seg_text.endswith(' ') and not simplified_text.endswith(' '):
                    simplified_text += ' '
                
                new_segs.append({"seg_id": seg['seg_id'], "text": simplified_text})
                new_text_parts.append(simplified_text)
            
            full_text = "".join(new_text_parts)
            doc = spacy(full_text)
            lemma_map = {t.text: helper.normalize_spanish_lemma(t.lemma_) for t in doc if not t.is_punct and not t.is_space}
            all_lemmas = {l for l in lemma_map.values() if l}
            
            for seg_data in new_segs:
                seg_doc = spacy(seg_data["text"])
                tokens = helper.create_golden_token_stream(seg_doc)
                seg_lemmas = {lemma_map.get(t['v']) for t in tokens if t['t'] == 'w' and lemma_map.get(t['v'])}
                for t in tokens:
                    if t['t'] == 'w' and (l := lemma_map.get(t['v'])):
                        t['l'] = [l]
                seg_data["tokenized_text"] = tokens
                seg_data["lemmas"] = sorted([l for l in seg_lemmas if l])

            output_content.append({"block_type": "sentence", "s_id": s_id, "full_text": full_text, "lemmas": sorted(list(all_lemmas)), "segments": new_segs})
        
        final_data = {"meta": {**std_data['meta'], "tier_type": tier_suffix}, "content": output_content}
        with open(derived_file_path, "w", encoding="utf-8") as f:
            json.dump(final_data, f, indent=2, ensure_ascii=False)
        logger.info(f"  -> Successfully saved '{derived_file_path.name}'.")
        if temp_path.exists(): temp_path.unlink()
        return derived_file_path

    # --- The functions below this point are for the older `.std.json` generation logic ---
    # They are not directly involved in the manual fix but are kept for completeness.
    def _get_or_create_std_json(self, required_lang: str) -> Optional[Path]:
        std_path = self.derived_texts_dir / f"{self.book_stem}.{required_lang}.std.json"
        if std_path.exists():
            logger.info(f"  -> Found existing asset: '{std_path.name}'")
            return std_path
        
        logger.info(f"  -> Asset '{std_path.name}' not found. Attempting to generate...")
        
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
        glob_pattern = f"{self.book_stem}.*.txt"
        found_files = list(self.source_texts_dir.glob(glob_pattern))
        if not found_files: return None
        
        source_file = found_files[0]
        parts = source_file.stem.split('.')
        if len(parts) > 1: return parts[-1], source_file
        return None

    def _translate_and_generate_std(self, book_stem: str, from_lang: str, to_lang: str) -> Optional[Path]:
        logger.info(f"    -> Translating '{book_stem}' from '{from_lang}' to '{to_lang}'...")
        source_std_path = self.derived_texts_dir / f"{book_stem}.{from_lang}.std.json"
        try:
            with open(source_std_path, 'r', encoding='utf-8') as f: source_data = json.load(f)
            source_items = source_data.get("content", [])
        except (IOError, json.JSONDecodeError) as e:
            logger.error(f"Could not read source .std.json for translation: {e}"); return None

        items_to_translate = [{"id": item['s_id'], "text": item['full_text']} for item in source_items if item.get('block_type') == 'sentence']
        
        temp_lang_config = {"base_code": from_lang, "target_code": to_lang, "manifest": self.lang_manifest, "pair_prompt_dir": None}
        pair_key = f"{from_lang}-{to_lang}"
        if pair_key in self.lang_manifest.get("pair", {}):
            temp_lang_config["pair_prompt_dir"] = self.lang_manifest["pair"][pair_key].get("prompt_dir")
        stage_job_config = self.stages_config.get("PoolManager_Translation", {})
        temp_path = self.derived_texts_dir / f"{book_stem}.{to_lang}.translation.temp.json"
        
        lang_name_from = self.lang_manifest.get(from_lang, {}).get("name", from_lang)
        lang_name_to = self.lang_manifest.get(to_lang, {}).get("name", to_lang)
        prompt = llm_prompts.get_system_prompt("translate_text", temp_lang_config).format(
            source_language_name=lang_name_from, target_language_name=lang_name_to
        )
        config = {**self.pipeline_config, **stage_job_config}
        
        translations = self._run_transactional_llm_job(
            f"Pool-Translation-{from_lang}-to-{to_lang}", prompt, items_to_translate, temp_path,
            LLMLogger(self.pool_dir / "llm_logs" / book_stem), "single_line", config, self.models_config
        )
        if translations is None: return None
        
        if len(translations) != len(items_to_translate):
            logger.error(f"Translation Integrity Check FAILED: Expected {len(items_to_translate)} translations, received {len(translations)}.")
            return None
        
        final_items = []
        for item in source_items:
            if item.get('block_type') == 'sentence':
                final_items.append({'type': 'sentence', 's_id': item['s_id'], 'text': translations.get(item['s_id'], "")})
            else:
                final_items.append({'type': 'chapter', 'text': item['text']})
        
        std_file = self.generate_std_file(book_stem, to_lang, translated_items=final_items)
        if std_file and temp_path.exists(): temp_path.unlink()
        return std_file

    def generate_std_file(self, book_stem: str, lang_code: str, translated_items: Optional[List[Dict]] = None) -> Optional[Path]:
        logger.info(f"  -> Generating pool file: '{book_stem}.{lang_code}.std.json'")
        std_file_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.std.json"
        
        if translated_items is not None:
            source_items = translated_items
        else:
            source_file_path = self.source_texts_dir / f"{book_stem}.{lang_code}.txt"
            source_items = self._parse_source_file(source_file_path)
        
        source_sentence_count = sum(1 for item in source_items if item.get('type') == 'sentence')
        stanza_processor = self.resources['stanza_processors'][lang_code]
        spacy_model = self.resources['spacy_models'][lang_code]
        output_content = []

        for item in source_items:
            if item['type'] == 'chapter':
                output_content.append({"block_type": "chapter", "text": item['text']})
                continue
            
            if item['type'] == 'sentence':
                s_id, original_text = item['s_id'], item['text']
                if not original_text.strip(): continue
                full_text = helper.preprocess_for_spacy(original_text)
                spacy_doc = spacy_model(full_text)
                segments_text = stanza_processor.segment_sentence(full_text)
                golden_stream = helper.create_golden_token_stream(spacy_doc)
                
                word_tokens = [tok for tok in golden_stream if tok['t'] == 'w']
                current_word_idx = 0
                for seg_idx, seg_str in enumerate(segments_text):
                    num_words = len(re.findall(r'\w+', seg_str))
                    for _ in range(num_words):
                        if current_word_idx < len(word_tokens): word_tokens[current_word_idx]['seg_idx'] = seg_idx
                        current_word_idx += 1
                
                num_segments = len(segments_text)
                if num_segments == 0: continue
                
                buckets: List[List[Dict]] = [[] for _ in range(num_segments)]
                b_idx = 0
                for token in golden_stream:
                    seg_idx = token.get('seg_idx', b_idx)
                    if seg_idx < num_segments:
                        buckets[seg_idx].append(token)
                        if token['t'] == 'w': b_idx = seg_idx
                
                if s_id == "S15": # Only print for our problem sentence
                    print(f"\n--- DEBUG START for {s_id} in pool_manager.py ---")
                    print("Tokens in buckets BEFORE boundary fix:")
                    for i, bucket in enumerate(buckets):
                        bucket_text = "".join(tok['v'] for tok in bucket)
                        print(f"  Bucket {i+1}: '{bucket_text}'")
                        if i < len(buckets) - 1 and buckets[i]:
                            print(f"    -> Last token: {buckets[i][-1]}")
                for i in range(num_segments - 1):
                    if buckets[i] and buckets[i+1]:
                        if buckets[i][-1]['t'] == 'w': buckets[i].append({'t':'b', 'v':''})
                        if buckets[i+1][0]['t'] == 'w': buckets[i+1].insert(0, {'t':'b', 'v':''})
                        b1, b2 = buckets[i][-1], buckets[i+1][0]
                        if s_id == "S15":
                            print(f"\n  Fixing boundary between Bucket {i+1} and {i+2}:")
                            print(f"    b1 (from Bucket {i+1}): {b1}")
                            print(f"    b2 (from Bucket {i+2}): {b2}")
                        combined = b1['v'] + b2['v']
                        split_idx = combined.find(' ')
                        if split_idx != -1: b1['v'], b2['v'] = combined[:split_idx + 1], combined[split_idx + 1:]
                        else: b1['v'], b2['v'] = combined, ""
                        if s_id == "S15":
                            print(f"    Combined: '{combined}' | Split Index: {split_idx}")
                            print(f"    NEW b1: {b1}")
                            print(f"    NEW b2: {b2}")

                if s_id == "S15":
                    print("\nTokens in buckets AFTER boundary fix:")
                    for i, bucket in enumerate(buckets):
                        bucket_text = "".join(tok['v'] for tok in bucket)
                        print(f"  Bucket {i+1}: '{bucket_text}'")
                    print(f"--- DEBUG END for {s_id} ---\n")
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
        
        output_sentence_count = sum(1 for block in output_content if block.get('block_type') == 'sentence')
        if source_sentence_count != output_sentence_count:
            logger.error(f"Integrity Check FAILED for '{std_file_path.name}': Source had {source_sentence_count} sentences, but output has {output_sentence_count}. Halting.")
            return None
        
        final_data = { "meta": { "book_name": book_stem, "language": lang_code, "tier_type": "std", "schema_version": "pool-v1.0" }, "content": output_content }
        try:
            with open(std_file_path, "w", encoding="utf-8") as f: json.dump(final_data, f, indent=2, ensure_ascii=False)
            logger.info(f"  -> Successfully saved '{std_file_path.name}'.")
            return std_file_path
        except IOError as e:
            logger.error(f"Failed to write .std.json file: {e}"); return None

    def _run_transactional_llm_job(self, job_name, system_prompt, all_items, temp_progress_path, llm_logger, parser_type, stage_config, models_config):
        completed = {}
        if temp_progress_path.exists():
            with open(temp_progress_path, 'r', encoding='utf-8') as f: completed = json.load(f)
        
        items_to_run = [i for i in all_items if i['id'] not in completed]
        if not items_to_run: return completed

        batch_size = stage_config.get("batch_size_in_items", 10)
        for i in range(0, len(items_to_run), batch_size):
            batch = items_to_run[i:i+batch_size]
            results = llm_utils.run_llm_batch_job(self.llm_client, job_name, system_prompt, batch, llm_logger, parser_type, stage_config, models_config)
            if not results: return None
            for item in results: completed[item['id']] = item['llm_response']
            with open(temp_progress_path, 'w', encoding='utf-8') as f: json.dump(completed, f, indent=2)
        return completed

    def _parse_source_file(self, file_path: Path) -> List[Dict[str, Any]]:
        text = file_path.read_text(encoding="utf-8")
        lines = text.splitlines()
        start = 1 if lines and lines[0].startswith("%%lang:") else 0
        items, s_re, c_re = [], re.compile(r"^{S(\d+):\s*(.*)}$"), re.compile(r"^%%CHAPTER_MARKER%%\s*(.*)$")
        for line in (l.strip() for l in lines[start:] if l.strip()):
            if (m := c_re.match(line)): items.append({"type": "chapter", "text": m.group(1).strip()})
            elif (m := s_re.match(line)): items.append({"type": "sentence", "s_id": f"S{int(m.group(1))}", "text": m.group(2).strip()})
        return items