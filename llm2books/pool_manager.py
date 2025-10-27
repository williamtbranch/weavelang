# llm2books/pool_manager.py
import json
import logging
import re
from pathlib import Path
from typing import Dict, Any, List, Optional

from . import helper, llm_prompts, llm_utils
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
        
        self.llm_clients = self.resources.get("llm_clients")
        self.lang_manifest = self.resources.get("language_config", {}).get("manifest", {})
        self.models_config = self.resources.get("models_config", {})
        self.pipeline_config = self.resources.get("pipeline_config", {})
        self.spacy_models = self.resources.get("spacy_models", {})
        self.stanza_processors = self.resources.get("stanza_processors", {})
        self.stages_config = self.resources.get("stages_config", {})

    def get_book_resources(self, book_stem: str, base_lang: str, target_lang: str) -> Optional[Dict[str, Path]]:
        """
        V11: The PoolManager is now only responsible for ensuring the foundational
        .std.json files exist for both the base and target languages.
        """
        logger.info(f"--- PoolManager (V11): Gathering foundational resources for '{book_stem}' ---")
        self.book_stem = book_stem
        
        try:
            # Get or create the base language .std.json (e.g., from the source .txt)
            base_std_path = self._get_or_create_std_json(base_lang)
            if not base_std_path: return None

            # Get or create the target language .std.json (by translating the base .std.json)
            target_std_path = self._get_or_create_std_json(target_lang)
            if not target_std_path: return None
            
            logger.info("--- PoolManager: Foundational resources are available. ---")
            return {
                "base_std": base_std_path,
                "target_std": target_std_path,
            }
        except FileNotFoundError as e:
            logger.error(f"Halting due to critical error in PoolManager: {e}")
            return None

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

    def generate_std_file(self, book_stem: str, lang_code: str, translated_items: Optional[List[Dict]] = None) -> Optional[Path]:
        # ... (implementation is unchanged) ...
        logger.info(f"  -> Generating pool file: '{book_stem}.{lang_code}.std.json'")
        std_file_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.std.json"
        source_items = translated_items if translated_items is not None else self._parse_source_file(self.source_texts_dir / f"{book_stem}.{lang_code}.txt")
        source_sentence_count = sum(1 for item in source_items if item.get('type') == 'sentence')
        stanza_processor = self.resources['stanza_processors'][lang_code]
        spacy_model = self.resources['spacy_models'][lang_code]
        output_content = []
        pool_llm_logger = LLMLogger(self.pool_dir / "llm_logs" / book_stem)
        original_logger = stanza_processor.llm_logger
        stanza_processor.llm_logger = pool_llm_logger
        try:
            for item in source_items:
                if item['type'] == 'chapter': output_content.append({"block_type": "chapter", "text": item['text']}); continue
                if item['type'] == 'sentence':
                    s_id, original_text = item['s_id'], item['text']
                    if not original_text.strip(): continue
                    full_text = helper.preprocess_for_spacy(original_text)
                    spacy_doc = spacy_model(full_text)
                    is_base_lang = (lang_code == self.resources['language_config']['base_code'])
                    segments_text = [full_text] if is_base_lang else stanza_processor.segment_sentence(full_text, s_id)
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
        finally:
            stanza_processor.llm_logger = original_logger
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

    def _run_transactional_llm_job(self, job_name, system_prompt, all_items, temp_progress_path, llm_logger, parser_type, stage_config, models_config, pipeline_config):
        # ... (implementation is unchanged) ...
        completed = {}
        if temp_progress_path.exists():
            with open(temp_progress_path, 'r', encoding='utf-8') as f: completed = json.load(f)
        items_to_run = [i for i in all_items if i['id'] not in completed]
        if not items_to_run: return completed
        batch_size = stage_config.get("batch_size_in_items", 10)
        for i in range(0, len(items_to_run), batch_size):
            batch = items_to_run[i:i+batch_size]
            results = llm_utils.run_llm_batch_job(self.llm_clients, job_name, system_prompt, batch, llm_logger, parser_type, stage_config, models_config, pipeline_config)
            if not results: return None
            for item in results: completed[item['id']] = item['llm_response']
            with open(temp_progress_path, 'w', encoding='utf-8') as f: json.dump(completed, f, indent=2)
        return completed

    #
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