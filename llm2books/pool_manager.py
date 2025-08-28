import json
import logging
import re
from pathlib import Path
from typing import Dict, Any, List, Optional
import time

from . import helper, llm_prompts, validator, llm_utils
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
        self.pool_dir.mkdir(exist_ok=True, parents=True)
        self.source_texts_dir.mkdir(exist_ok=True, parents=True)
        self.llm_client = self.resources.get("llm_client")
        self.lang_manifest = self.resources.get("language_config", {}).get("manifest", {})
        self.models_config = self.resources.get("models_config", {})
        self.pipeline_config = self.resources.get("pipeline_config", {})
        self.spacy_models = self.resources.get("spacy_models", {})
        self.stanza_processors = self.resources.get("stanza_processors", {})

    def get_book_resources(self, book_stem: str, base_lang: str, target_lang: str) -> Optional[Dict[str, Path]]:
        """
        Main entry point that ensures all required pool assets for a pipeline run exist,
        generating them via a lazy-loading, dependency-aware process.
        """
        logger.info(f"--- PoolManager: Gathering resources for '{book_stem}' ({base_lang} -> {target_lang}) ---")
        self.book_stem = book_stem # Store book_stem for helper methods
        
        try:
            # --- THIS IS THE FIX ---
            # Each call is now checked immediately. If any fails, the function returns None.
            base_std_path = self._get_or_create_std_json(base_lang)
            if not base_std_path: return None

            target_std_path = self._get_or_create_std_json(target_lang)
            if not target_std_path: return None

            target_sim_path = self._get_or_create_sim_json(target_lang)
            if not target_sim_path: return None
            # --- END OF FIX ---
            
            logger.info("--- PoolManager: All required resources are now available. ---")
            return {
                "base_std": base_std_path,
                "target_std": target_std_path,
                "target_sim": target_sim_path,
            }
        except FileNotFoundError as e:
            logger.error(f"Halting due to critical error: {e}")
            return None


    def _get_or_create_std_json(self, required_lang: str) -> Optional[Path]:
        """Lazy getter for a .std.json file. Handles generation and translation dependencies."""
        std_path = self.derived_texts_dir / f"{self.book_stem}.{required_lang}.std.json"
        if std_path.exists():
            logger.info(f"  -> Found existing asset: '{std_path.name}'")
            return std_path
        
        logger.info(f"  -> Asset '{std_path.name}' not found. Attempting to generate...")
        
        # Find the ultimate source text
        source_info = self._find_true_source_file()
        if not source_info:
            raise FileNotFoundError(f"Source text file for '{self.book_stem}' not found in {self.source_texts_dir}.")
        true_source_lang, _ = source_info
        
        if true_source_lang == required_lang:
            # The source is already the correct language, just generate it.
            return self.generate_std_file(self.book_stem, required_lang)
        else:
            # We need a translation. First, recursively ensure the source .std.json exists.
            logger.info(f"    -> Dependency: '{std_path.name}' requires '{true_source_lang}' source. Checking for '{self.book_stem}.{true_source_lang}.std.json'...")
            source_std_path = self._get_or_create_std_json(true_source_lang)
            if not source_std_path:
                logger.error(f"Failed to generate dependency '{true_source_lang}.std.json' for translation.")
                return None
            
            # Now that the dependency is met, perform the translation.
            return self._translate_and_generate_std(self.book_stem, true_source_lang, required_lang)

    def _get_or_create_sim_json(self, target_lang: str) -> Optional[Path]:
        """Lazy getter for a .sim.json file."""
        sim_path = self.derived_texts_dir / f"{self.book_stem}.{target_lang}.sim.json"
        if sim_path.exists():
            logger.info(f"  -> Found existing asset: '{sim_path.name}'")
            return sim_path
        
        logger.info(f"  -> Asset '{sim_path.name}' not found. Attempting to generate...")
        
        # Dependency: requires the .std.json file of the same language
        logger.info(f"    -> Dependency: '{sim_path.name}' requires '{self.book_stem}.{target_lang}.std.json'.")
        target_std_path = self._get_or_create_std_json(target_lang)
        if not target_std_path:
            logger.error(f"Failed to generate dependency '{target_lang}.std.json' for simplification.")
            return None
            
        return self.generate_sim_file(self.book_stem, target_lang)

    def _find_true_source_file(self) -> Optional[tuple[str, Path]]:
        """Scans for the source text file for the current book_stem."""
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
        
        # --- THIS IS THE FIX ---
        # Build a temporary language_config for this specific translation pair
        temp_lang_config = {"base_code": from_lang, "target_code": to_lang, "manifest": self.lang_manifest, "pair_prompt_dir": None}
        pair_key = f"{from_lang}-{to_lang}"
        if pair_key in self.lang_manifest.get("pair", {}):
            temp_lang_config["pair_prompt_dir"] = self.lang_manifest["pair"][pair_key].get("prompt_dir")
        # --- END OF FIX ---

        temp_path = self.derived_texts_dir / f"{book_stem}.{to_lang}.translation.temp.json"
        prompt = llm_prompts.get_system_prompt("translate_text", temp_lang_config).format(
            source_language_name=self.lang_manifest.get(from_lang, {}).get("name", from_lang),
            target_language_name=self.lang_manifest.get(to_lang, {}).get("name", to_lang)
        )
        config = {"primary_model": "sonnet", "fallback_model": "sonnet", **self.pipeline_config}

        translations = self._run_transactional_llm_job(
            f"Pool-Translation-{from_lang}-to-{to_lang}", prompt, items_to_translate, temp_path,
            LLMLogger(self.pool_dir / "llm_logs" / book_stem), "single_line", config, self.models_config
        )
        if translations is None: return None
        
        # --- THIS IS THE FIX ---
        # Integrity check the translation results *before* trying to generate the file.
        source_sentence_count = len(items_to_translate)
        if len(translations) != source_sentence_count:
            logger.error(f"Translation Integrity Check FAILED: Expected {source_sentence_count} translations, but received {len(translations)}.")
            return None
        logger.info("Translation Integrity Check PASSED.")
        # --- END OF FIX ---

        final_items = [
            ({'type': 'sentence', 's_id': item['s_id'], 'text': translations.get(item['s_id'], "")}
             if item.get('block_type') == 'sentence' else {'type': 'chapter', 'text': item['text']}) # Ensure type is set
            for item in source_items
        ]
        
        std_file = self.generate_std_file(book_stem, to_lang, translated_items=final_items)
        if std_file and temp_path.exists(): temp_path.unlink()
        return std_file

    # --- The following methods (generate_std_file, generate_sim_file, etc.)
    # can remain exactly as they were in our last fully working version.
    # I am including them here for completeness. ---

    def generate_std_file(self, book_stem: str, lang_code: str, translated_items: Optional[List[Dict]] = None) -> Optional[Path]:
        logger.info(f"  -> Generating pool file: '{book_stem}.{lang_code}.std.json'")
        std_file_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.std.json"
        
        if translated_items is not None:
            source_items = translated_items
        else:
            source_file_path = self.source_texts_dir / f"{book_stem}.{lang_code}.txt"
            source_items = self._parse_source_file(source_file_path)
        
        source_sentence_count = sum(1 for item in source_items if item.get('type') == 'sentence')

        stanza_processor: StanzaLanguageProcessor = self.resources['stanza_processors'][lang_code]
        spacy_model = self.resources['spacy_models'][lang_code]
            
        output_content = []

        for item in source_items:
            if item['type'] == 'chapter':
                output_content.append({"block_type": "chapter", "text": item['text']})
                continue
            
            if item['type'] == 'sentence':
                s_id, full_text = item['s_id'], item['text']
                if not full_text.strip(): continue

                spacy_doc = spacy_model(full_text)
                golden_stream = helper.create_golden_token_stream(full_text, spacy_doc)
                segments_text = stanza_processor.segment_sentence(full_text)
                
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
                                    lemma = helper.normalize_spanish_lemma(st.lemma_)
                                    if lemma: token['l'] = [lemma]; all_lemmas.add(lemma); seg_lemmas.add(lemma)
                                    break
                    segments_data.append({ "seg_id": f"S{i+1}", "text": seg_text, "tokenized_text": bucket, "lemmas": sorted(list(seg_lemmas))})
                
                output_content.append({ "block_type": "sentence", "s_id": s_id, "full_text": full_text, "lemmas": sorted(list(all_lemmas)), "segments": segments_data })

        output_sentence_count = sum(1 for block in output_content if block.get('block_type') == 'sentence')
        if source_sentence_count != output_sentence_count:
            logger.error(f"Integrity Check FAILED for '{std_file_path.name}': Source had {source_sentence_count} sentences, but output has {output_sentence_count}. Halting.")
            return None
        logger.info(f"Integrity Check PASSED for '{std_file_path.name}': Processed {output_sentence_count} sentences.")
        
        final_data = { "meta": { "book_name": book_stem, "language": lang_code, "tier_type": "std", "schema_version": "pool-v1.0" }, "content": output_content }
        try:
            with open(std_file_path, "w", encoding="utf-8") as f: json.dump(final_data, f, indent=2, ensure_ascii=False)
            logger.info(f"  -> Successfully saved '{std_file_path.name}'.")
            return std_file_path
        except IOError as e:
            logger.error(f"Failed to write .std.json file: {e}"); return None

    def generate_sim_file(self, book_stem: str, lang_code: str) -> Optional[Path]:
        sim_file_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.sim.json"
        std_target_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.std.json"
        with open(std_target_path, 'r', encoding='utf-8') as f: std_data = json.load(f)

        sentences = [b for b in std_data.get("content", []) if b.get("block_type") == "sentence"]
        if not sentences:
            logger.warning("Source has no sentences. Creating empty sim file.")
            with open(sim_file_path, "w", encoding="utf-8") as f: json.dump({**std_data, "content": [b for b in std_data.get("content", []) if b.get("block_type") != "sentence"]}, f, indent=2, ensure_ascii=False)
            return sim_file_path

        items_to_simplify = [{"id": f"{s['s_id']}_{seg['seg_id']}", "text": seg["text"]} for s in sentences for seg in s["segments"] if seg["text"].strip()]
        
        temp_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.simplify.temp.json"
        prompt = llm_prompts.get_system_prompt("simplify_segments", self.resources["language_config"])
        config = {"primary_model": "sonnet", "fallback_model": "sonnet", **self.pipeline_config}
        
        results = self._run_transactional_llm_job(
            "Pool-Simplification", prompt, items_to_simplify, temp_path,
            LLMLogger(self.pool_dir / "llm_logs" / book_stem), "single_line", config, self.models_config
        )
        if results is None: return None
        
        spacy = self.resources['spacy_models'][lang_code]
        output_content = [b for b in std_data.get("content", []) if b.get("block_type") != "sentence"]

        for s_block in sentences:
            s_id = s_block['s_id']
            new_segs, new_text_parts = [], []
            for seg in s_block["segments"]:
                original_seg_text = seg["text"]
                new_seg_text = results.get(f"{s_id}_{seg['seg_id']}", original_seg_text)
                if original_seg_text.endswith(' ') and not new_seg_text.endswith(' '): new_seg_text += ' '
                new_segs.append({"seg_id": seg['seg_id'], "text": new_seg_text})
                new_text_parts.append(new_seg_text)
            
            full_text = "".join(new_text_parts)
            doc = spacy(full_text)
            lemma_map = {t.text: helper.normalize_spanish_lemma(t.lemma_) for t in doc if not t.is_punct and not t.is_space}
            all_lemmas = {l for l in lemma_map.values() if l}
            
            for seg_data in new_segs:
                seg_doc = spacy(seg_data["text"])
                tokens = helper.create_golden_token_stream(seg_data["text"], seg_doc)
                seg_lemmas = {lemma_map.get(t['v']) for t in tokens if t['t'] == 'w' and lemma_map.get(t['v'])}
                for t in tokens:
                    if t['t'] == 'w' and (l := lemma_map.get(t['v'])): t['l'] = [l]
                seg_data["tokenized_text"] = tokens
                seg_data["lemmas"] = sorted([l for l in seg_lemmas if l])

            output_content.append({"block_type": "sentence", "s_id": s_id, "full_text": full_text, "lemmas": sorted(list(all_lemmas)), "segments": new_segs})
        
        final_data = {"meta": {**std_data['meta'], "tier_type": "sim"}, "content": output_content}
        with open(sim_file_path, "w", encoding="utf-8") as f: json.dump(final_data, f, indent=2, ensure_ascii=False)
        logger.info(f"  -> Successfully saved '{sim_file_path.name}'.")
        if temp_path.exists(): temp_path.unlink()
        return sim_file_path

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
