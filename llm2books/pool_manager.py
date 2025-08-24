# In llm2books/pool_manager.py

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
    # ... (The __init__, get_book_resources, _translate_and_generate_std,
    #      _run_transactional_llm_job, and _parse_source_file methods are all correct
    #      and can remain as they were in the last fully working version I sent.
    #      The bug is solely within generate_std_file and generate_sim_file) ...

    # For clarity, here is the full correct file.

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
        logger.info(f"--- PoolManager: Gathering resources for '{book_stem}' ({base_lang} -> {target_lang}) ---")
        base_std_path = self.derived_texts_dir / f"{book_stem}.{base_lang}.std.json"
        target_std_path = self.derived_texts_dir / f"{book_stem}.{target_lang}.std.json"
        target_sim_path = self.derived_texts_dir / f"{book_stem}.{target_lang}.sim.json"
        if not base_std_path.exists():
            if not self.generate_std_file(book_stem, base_lang): return None
        if not target_std_path.exists():
            if not self._translate_and_generate_std(book_stem, base_lang, target_lang): return None
        if not target_sim_path.exists():
            if not self.generate_sim_file(book_stem, target_lang): return None
        logger.info("--- PoolManager: All required resources are available. ---")
        return {"base_std": base_std_path, "target_std": target_std_path, "target_sim": target_sim_path}

    def _translate_and_generate_std(self, book_stem: str, from_lang: str, to_lang: str) -> Optional[Path]:
        logger.info(f"    -> Translating '{book_stem}' from '{from_lang}' to '{to_lang}'...")
        source_path = self.source_texts_dir / f"{book_stem}.{from_lang}.txt"
        source_items = self._parse_source_file(source_path)
        items_to_translate = [{"id": item['s_id'], "text": item['text']} for item in source_items if item['type'] == 'sentence']
        
        temp_path = self.derived_texts_dir / f"{book_stem}.{to_lang}.translation.temp.json"
        prompt = llm_prompts.get_system_prompt("translate_text", self.resources["language_config"]).format(
            source_language_name=self.lang_manifest.get(from_lang, {}).get("name", from_lang),
            target_language_name=self.lang_manifest.get(to_lang, {}).get("name", to_lang)
        )
        config = {"primary_model": "sonnet", "fallback_model": "sonnet", **self.pipeline_config}

        translations = self._run_transactional_llm_job(
            f"Pool-Translation", prompt, items_to_translate, temp_path,
            LLMLogger(self.pool_dir / "llm_logs" / book_stem), "single_line", config, self.models_config
        )
        if translations is None: return None
        
        if len(translations) != len(items_to_translate):
            logger.error("Integrity Check FAILED: Translation count mismatch."); return None
        logger.info("Integrity Check PASSED: Translation count matches source count.")

        final_items = [
            ({'type': 'sentence', 's_id': item['s_id'], 'text': translations.get(item['s_id'], "")} 
             if item['type'] == 'sentence' else item) for item in source_items
        ]
        
        std_file = self.generate_std_file(book_stem, to_lang, translated_items=final_items)
        if std_file and temp_path.exists(): temp_path.unlink()
        return std_file

    def generate_std_file(self, book_stem: str, lang_code: str, translated_items: Optional[List[Dict]] = None) -> Optional[Path]:
        logger.info(f"  -> Generating pool file: '{book_stem}.{lang_code}.std.json'")
        std_file_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.std.json"
        
        source_items = translated_items if translated_items is not None else self._parse_source_file(self.source_texts_dir / f"{book_stem}.{lang_code}.txt")
        
        stanza_processor: StanzaLanguageProcessor = self.resources['stanza_processors'][lang_code]
        spacy_model = self.resources['spacy_models'][lang_code]
            
        output_content = []
        source_sentence_count = sum(1 for item in source_items if item.get('type') == 'sentence')

        for item in source_items:
            if item['type'] == 'chapter':
                output_content.append({"block_type": "chapter", "text": item['text']})
                continue
            
            if item['type'] == 'sentence':
                s_id, full_text = item['s_id'], item['text']
                if not full_text.strip(): continue

                # --- THIS IS THE CORRECTED LOGIC ---
                # 1. Use SpaCy to create the spacy_doc. This is our source of truth for tokens.
                spacy_doc = spacy_model(full_text)
                # 2. Pass the spacy_doc to the helper. This avoids Stanza's tokenizer entirely.
                golden_stream = helper.create_golden_token_stream(full_text, spacy_doc)
                
                # 3. Use Stanza ONLY for its high-quality segmentation.
                segments_text = stanza_processor.segment_sentence(full_text)
                # --- END OF CORRECTED LOGIC ---
                
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

    # ... The generate_sim_file and other helpers are also included in the full replacement ...
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
                new_seg_text = results.get(f"{s_id}_{seg['seg_id']}", seg["text"])
                if seg["text"].endswith(' ') and not new_seg_text.endswith(' '): new_seg_text += ' '
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