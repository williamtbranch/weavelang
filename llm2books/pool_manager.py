# In llm2books/pool_manager.py

import json
import logging
import re
from pathlib import Path
from typing import Dict, Any, List, Optional
import time

from . import helper
from . import llm_prompts
from . import validator
from .stanza_segmenter import StanzaLanguageProcessor
from .llm_logger import LLMLogger
# --- NEW: Import the LLM utility module ---
from . import llm_utils

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
        # --- NEW: Store models and pipeline config for easy access ---
        self.models_config = self.resources.get("models_config", {})
        self.pipeline_config = self.resources.get("pipeline_config", {})

    def get_book_resources(self, book_stem: str, base_lang: str, target_lang: str) -> Optional[Dict[str, Path]]:
        logger.info(f"--- PoolManager: Gathering resources for '{book_stem}' ({base_lang} -> {target_lang}) ---")
        base_std_path = self.derived_texts_dir / f"{book_stem}.{base_lang}.std.json"
        target_std_path = self.derived_texts_dir / f"{book_stem}.{target_lang}.std.json"
        target_sim_path = self.derived_texts_dir / f"{book_stem}.{target_lang}.sim.json"

        # This logic remains the same, but the functions it calls are now fixed.
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

    # ... generate_std_file and its helpers remain unchanged ...
    # (I'm omitting the unchanged code for brevity, but it should be kept in your file)
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
            if item['type'] == 'chapter':
                output_content.append({ "block_type": "chapter", "text": item['text'] })
                continue

            s_id, full_text = item['s_id'], item['text']
            
            spacy_doc = spacy_model(full_text)
            golden_stream = helper.create_golden_token_stream(full_text, spacy_doc)
            
            stanza_doc = stanza_processor.nlp(full_text)
            if not stanza_doc.sentences:
                continue
            
            tree = stanza_doc.sentences[0].constituency
            hierarchical_strings = []
            if tree and tree.children:
                initial_tuples = stanza_processor._get_hierarchical_segments(tree.children[0])
                initial_strings = [text for text, _ in initial_tuples]
                hierarchical_strings = stanza_processor._refine_for_quotes(initial_strings)

            word_tokens_in_stream = [tok for tok in golden_stream if tok['t'] == 'w']
            current_word_idx = 0
            for seg_idx, seg_str in enumerate(hierarchical_strings):
                num_words_in_seg = len(re.findall(r'\w+', seg_str))
                for _ in range(num_words_in_seg):
                    if current_word_idx < len(word_tokens_in_stream):
                        word_tokens_in_stream[current_word_idx]['seg_idx'] = seg_idx
                        current_word_idx += 1
            
            num_segments = len(hierarchical_strings)
            segment_buckets: List[List[Dict]] = [[] for _ in range(num_segments)]
            current_seg_idx_for_b = 0
            for token in golden_stream:
                if token['t'] == 'w':
                    seg_idx = token.get('seg_idx', current_seg_idx_for_b)
                    if seg_idx < num_segments:
                        segment_buckets[seg_idx].append(token)
                        current_seg_idx_for_b = seg_idx
                else:
                    if current_seg_idx_for_b < num_segments:
                        segment_buckets[current_seg_idx_for_b].append(token)

            for i in range(num_segments - 1):
                if not segment_buckets[i] or not segment_buckets[i+1]: continue
                if segment_buckets[i][-1]['t'] == 'w': segment_buckets[i].append({'t':'b', 'v':''})
                if segment_buckets[i+1][0]['t'] == 'w': segment_buckets[i+1].insert(0, {'t':'b', 'v':''})
                b1 = segment_buckets[i][-1]
                b2 = segment_buckets[i+1][0]
                combined = b1['v'] + b2['v']
                split_point = combined.find(' ')
                if split_point != -1:
                    b1['v'] = combined[:split_point + 1]
                    b2['v'] = combined[split_point + 1:]
                else:
                    b1['v'] = combined
                    b2['v'] = ""
            
            segments_data = []
            sentence_di_counter = 0
            all_sentence_lemmas = set()
            
            for i, bucket in enumerate(segment_buckets):
                seg_text = "".join(tok['v'] for tok in bucket)
                seg_doc = spacy_model(seg_text)
                
                seg_lemmas = set()
                token_char_map = { t.idx: t for t in seg_doc }

                for token in bucket:
                    if token.get('t') == 'w':
                        token['di'] = sentence_di_counter
                        sentence_di_counter += 1
                        
                        char_pos = seg_text.find(token['v'])
                        spacy_token = token_char_map.get(char_pos)

                        if spacy_token:
                            norm_lemma = helper.normalize_spanish_lemma(spacy_token.lemma_)
                            if norm_lemma:
                                token['l'] = [norm_lemma]
                                all_sentence_lemmas.add(norm_lemma)
                                seg_lemmas.add(norm_lemma)
                
                segments_data.append({
                    "seg_id": f"S{i+1}",
                    "text": seg_text,
                    "tokenized_text": bucket,
                    "lemmas": sorted(list(seg_lemmas))
                })

            output_content.append({
                "block_type": "sentence",
                "s_id": s_id,
                "full_text": full_text,
                "lemmas": sorted(list(l for l in all_sentence_lemmas if l)),
                "segments": segments_data
            })

        logger.info(f"  -> Validating generated std data for '{book_stem}.{lang_code}'...")
        try:
            for sentence_block in output_content:
                if sentence_block.get("block_type") == "sentence":
                    temp_tier = { "tier_id": f"std-pool-{lang_code}", "full_text": sentence_block.get("full_text", ""), "segments": sentence_block.get("segments", []) }
                    validator.validate_segment_reconstruction(temp_tier)
                    for seg in sentence_block.get("segments", []):
                        reconstructed_from_tokens = "".join(t['v'] for t in seg.get("tokenized_text", []))
                        if reconstructed_from_tokens != seg.get("text"):
                            raise validator.ValidationError(f"Token reconstruction for s_id {sentence_block['s_id']} seg_id {seg['seg_id']} failed.")
        except validator.ValidationError as e:
            logger.error(f"  -> CRITICAL: Validation failed for pool file '{book_stem}.{lang_code}.std.json'. Halting generation.")
            logger.error(f"     Reason: {e}")
            return None
        
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
        all_items_to_translate = [{"id": item['s_id'], "text": item['text']} for item in source_items if item['type'] == 'sentence']

        temp_translation_path = self.derived_texts_dir / f"{book_stem}.{to_lang}.translation.temp.json"
        completed_translations = {}
        if temp_translation_path.exists():
            try:
                with open(temp_translation_path, 'r', encoding='utf-8') as f:
                    completed_translations = json.load(f)
                logger.info(f"      -> Resuming translation. Found {len(completed_translations)} completed items.")
            except (IOError, json.JSONDecodeError):
                logger.warning("      -> Corrupt temp translation file found. Starting from scratch.")

        items_for_this_run = [item for item in all_items_to_translate if item['id'] not in completed_translations]

        if not items_for_this_run:
            logger.info("      -> Translation is already complete.")
        else:
            logger.info(f"      -> Translating {len(items_for_this_run)} new items.")
            from_lang_name = self.lang_manifest.get(from_lang, {}).get("name", from_lang)
            to_lang_name = self.lang_manifest.get(to_lang, {}).get("name", to_lang)
            
            system_prompt_template = llm_prompts.get_system_prompt("translate_text", self.resources["language_config"])
            system_prompt = system_prompt_template.format(source_language_name=from_lang_name, target_language_name=to_lang_name)
            llm_logger = LLMLogger(self.pool_dir / "llm_logs" / book_stem)
            
            # --- MODIFIED: Create a temporary config for this specific job ---
            temp_stage_config = {
                "primary_model": "sonnet", # Good, fast choice for translation
                "fallback_model": "sonnet",
                "max_api_retries": self.pipeline_config.get("max_api_retries", 3),
                "retry_delay": self.pipeline_config.get("retry_delay", 7),
                "batch_size_in_items": 10
            }

            for i in range(0, len(items_for_this_run), temp_stage_config["batch_size_in_items"]):
                batch_num = (i // temp_stage_config["batch_size_in_items"]) + 1
                batch = items_for_this_run[i:i + temp_stage_config["batch_size_in_items"]]
                
                # --- MODIFIED: Pass the new required arguments ---
                batch_results = llm_utils.run_llm_batch_job(
                    llm_client=self.llm_client,
                    job_name=f"Pool-Translation-{from_lang}-to-{to_lang}",
                    system_prompt=system_prompt,
                    items_to_process=batch,
                    llm_logger=llm_logger,
                    parser_type="single_line",
                    stage_config=temp_stage_config,
                    models_config=self.models_config
                )

                if not batch_results:
                    logger.error(f"LLM batch #{batch_num} failed permanently. Progress up to this point has been saved. Halting job.")
                    return None

                for item in batch_results:
                    completed_translations[item['id']] = item['llm_response']
                
                try:
                    with open(temp_translation_path, 'w', encoding='utf-8') as f:
                        json.dump(completed_translations, f, indent=2)
                    logger.info(f"      -> Successfully saved progress for batch #{batch_num} to temp file.")
                except IOError as e:
                    logger.error(f"CRITICAL: Could not save temp translation progress: {e}")
                    return None

        final_items_for_std_gen = []
        for item in source_items:
            if item['type'] == 'sentence':
                final_items_for_std_gen.append({
                    'type': 'sentence', 's_id': item['s_id'],
                    'text': completed_translations.get(item['s_id'], "TRANSLATION_MISSING")
                })
            elif item['type'] == 'chapter':
                final_items_for_std_gen.append(item)
        
        std_file = self.generate_std_file(book_stem, to_lang, translated_items=final_items_for_std_gen)
        
        if std_file and temp_translation_path.exists():
            temp_translation_path.unlink()
            
        return std_file

    def generate_sim_file(self, book_stem: str, lang_code: str) -> Optional[Path]:
        logger.info(f"  -> Generating pool file: '{book_stem}.{lang_code}.sim.json'")
        sim_file_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.sim.json"
        std_target_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.std.json"
        
        if not std_target_path.exists():
            logger.error(f"Cannot generate .sim file: Required parent .std file not found."); return None
        try:
            with open(std_target_path, 'r', encoding='utf-8') as f: std_target_data = json.load(f)
        except Exception as e:
            logger.error(f"Failed to read or parse parent .std file: {e}"); return None

        llm_logger = LLMLogger(self.pool_dir / "llm_logs" / book_stem)
        
        # (The logic for handling empty files and preparing items is unchanged)
        all_content_blocks = std_target_data.get("content", [])
        if not all_content_blocks:
            logger.error(f"The source file {std_target_path.name} contains no content blocks.")
            return None

        target_sentences = [block for block in all_content_blocks if block.get("block_type") == "sentence"]
        if not target_sentences:
            logger.warning(f"The source file {std_target_path.name} contains content, but no blocks of type 'sentence'.")
            final_json_data = {"meta": {"book_name": book_stem, "language": lang_code, "tier_type": "sim", "schema_version": "pool-v1.0", "parent_std_file": std_target_path.name}, "content": [b for b in all_content_blocks if b.get("block_type") != "sentence"]}
            try:
                with open(sim_file_path, "w", encoding="utf-8") as f: json.dump(final_json_data, f, indent=2, ensure_ascii=False)
                return sim_file_path
            except IOError as e:
                logger.error(f"Failed to write empty .sim.json file: {e}"); return None

        all_items_to_simplify = []
        for s in target_sentences:
            for seg in s.get("segments", []):
                text_to_simplify = seg.get("text", "")
                if text_to_simplify.strip():
                    all_items_to_simplify.append({ "id": f"{s['s_id']}_{seg['seg_id']}", "text": text_to_simplify })

        # --- MODIFIED: Create a temporary config for the simplification job ---
        temp_stage_config_simplify = {
            "primary_model": "sonnet", # Good default for simplification
            "fallback_model": "sonnet",
            "max_api_retries": self.pipeline_config.get("max_api_retries", 3),
            "retry_delay": self.pipeline_config.get("retry_delay", 7),
            "batch_size_in_items": 10
        }
        
        temp_simplify_path = self.derived_texts_dir / f"{book_stem}.{lang_code}.simplify.temp.json"
        # --- MODIFIED: Pass the new configs to the transactional helper ---
        simplified_results_map = self._run_transactional_llm_job(
            job_name="Pool-Simplification",
            system_prompt=llm_prompts.get_system_prompt("simplify_segments", self.resources["language_config"]),
            all_items=all_items_to_simplify,
            temp_progress_path=temp_simplify_path,
            llm_logger=llm_logger,
            parser_type="single_line",
            stage_config=temp_stage_config_simplify,
            models_config=self.models_config
        )
        if simplified_results_map is None: return None
        
        # (The rest of the function for processing and saving the .sim.json file is unchanged)
        output_content = []
        spacy_model = self.spacy_models.get(lang_code)
        for block in std_target_data.get("content", []):
            if block.get("block_type") != "sentence":
                output_content.append(block)
                continue
            s_id = block['s_id']
            new_segments_data = []
            new_full_text_parts = []
            for seg in block.get("segments", []):
                lookup_key = f"{s_id}_{seg['seg_id']}"
                original_seg_text = seg.get("text", "")
                new_seg_text = simplified_results_map.get(lookup_key, original_seg_text)
                if original_seg_text.endswith(' ') and not new_seg_text.endswith(' '):
                    new_seg_text += ' '
                new_segments_data.append({"seg_id": seg['seg_id'], "text": new_seg_text})
                new_full_text_parts.append(new_seg_text)
            simpler_full_text = "".join(new_full_text_parts)
            full_doc = spacy_model(simpler_full_text)
            token_to_lemma = {token.text: helper.normalize_spanish_lemma(token.lemma_) for token in full_doc if not token.is_punct and not token.is_space}
            all_sentence_lemmas = {lemma for lemma in token_to_lemma.values() if lemma}
            for seg_data in new_segments_data:
                seg_doc = spacy_model(seg_data["text"])
                token_list = helper.create_golden_token_stream(seg_data["text"], seg_doc)
                seg_lemmas = {token_to_lemma.get(t['v']) for t in token_list if t['t'] == 'w' and token_to_lemma.get(t['v'])}
                for token in token_list:
                    if token['t'] == 'w':
                        lemma = token_to_lemma.get(token['v'])
                        if lemma: token['l'] = [lemma]
                seg_data["tokenized_text"] = token_list
                seg_data["lemmas"] = sorted(list(l for l in seg_lemmas if l))
            output_content.append({ "block_type": "sentence", "s_id": s_id, "full_text": simpler_full_text, "lemmas": sorted(list(all_sentence_lemmas)), "segments": new_segments_data })
        final_meta = { "book_name": book_stem, "language": lang_code, "tier_type": "sim", "schema_version": "pool-v1.0", "parent_std_file": std_target_path.name }
        final_json_data = {"meta": final_meta, "content": output_content}
        try:
            logger.info(f"  -> Validating final generated sim data for '{book_stem}.{lang_code}'...")
            for sentence_block in output_content:
                if sentence_block.get("block_type") == "sentence":
                    temp_tier = {"tier_id": "sim-pool-final", "full_text": sentence_block.get("full_text"), "segments": sentence_block.get("segments")}
                    validator.validate_segment_reconstruction(temp_tier)
                    for seg in sentence_block.get("segments", []):
                        reconstructed = "".join(t['v'] for t in seg.get("tokenized_text", []))
                        if reconstructed != seg.get("text"):
                            raise validator.ValidationError(f"Token reconstruction for s_id {sentence_block['s_id']} seg_id {seg['seg_id']} failed.")
            with open(sim_file_path, "w", encoding="utf-8") as f: json.dump(final_json_data, f, indent=2, ensure_ascii=False)
            logger.info(f"  -> Successfully saved '{sim_file_path.name}' to common pool.")
            if temp_simplify_path.exists(): temp_simplify_path.unlink()
            return sim_file_path
        except (IOError, validator.ValidationError) as e:
            logger.error(f"Failed to write or validate .sim.json file to {sim_file_path}: {e}"); return None

    # --- MODIFIED: The signature of this helper now includes the config dictionaries ---
    def _run_transactional_llm_job(self, job_name: str, system_prompt: str, all_items: List[Dict], temp_progress_path: Path, llm_logger: Any, parser_type: str, stage_config: Dict, models_config: Dict) -> Optional[Dict[str, str]]:
        completed_items = {}
        if temp_progress_path.exists():
            try:
                with open(temp_progress_path, 'r', encoding='utf-8') as f:
                    completed_items = json.load(f)
                logger.info(f"      -> Resuming {job_name}. Found {len(completed_items)} completed items.")
            except (IOError, json.JSONDecodeError):
                logger.warning(f"      -> Corrupt temp file for {job_name}. Starting from scratch.")

        items_for_this_run = [item for item in all_items if item['id'] not in completed_items]

        if not items_for_this_run:
            logger.info(f"      -> {job_name} is already complete.")
            return completed_items

        batch_size = stage_config.get("batch_size_in_items", 10)
        for i in range(0, len(items_for_this_run), batch_size):
            batch_num = (i // batch_size) + 1
            batch = items_for_this_run[i:i + batch_size]
            
            # --- MODIFIED: Pass the configs down to the utility function ---
            batch_results = llm_utils.run_llm_batch_job(
                llm_client=self.llm_client,
                job_name=job_name,
                system_prompt=system_prompt,
                items_to_process=batch,
                llm_logger=llm_logger,
                parser_type=parser_type,
                stage_config=stage_config,
                models_config=models_config
            )

            if not batch_results:
                logger.error(f"LLM batch #{batch_num} for {job_name} failed permanently. Progress saved. Halting.")
                return None

            for item in batch_results:
                completed_items[item['id']] = item['llm_response']
            
            try:
                with open(temp_progress_path, 'w', encoding='utf-8') as f:
                    json.dump(completed_items, f, indent=2)
            except IOError as e:
                logger.error(f"CRITICAL: Could not save temp progress for {job_name}: {e}")
                return None
        
        return completed_items

    # (The _parse_source_file function is unchanged)
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