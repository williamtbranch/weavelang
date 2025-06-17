# Filename: orchestrate_pipeline.py
# Purpose: Main entry point and unified orchestrator for the WeaveLang data generation pipeline.
# Description: This script implements a robust, resumable, depth-first processing strategy.
# It includes intra-stage JSON saves and universal validation/retry logic for all LLM stages.

import sys
import argparse
from pathlib import Path
import json
import time
import re
from typing import Dict, Any, List, Optional

from helper import Helper
from stage1 import Stage1

Helper = Helper()

# --- Library Imports ---
try:
    import tomllib
except ImportError:
    try:
        import toml as tomllib
        print("INFO: Using 'toml' library for config. Python 3.11+ with 'tomllib' is preferred.", file=sys.stderr)
    except ImportError:
        print("CRITICAL: TOML library not found. Please `pip install toml` or use Python 3.11+.", file=sys.stderr)
        sys.exit(1)
try:
    import spacy
except ImportError:
    print("CRITICAL: SpaCy library not found. Please run `pip install spacy`.", file=sys.stderr)
    sys.exit(1)


# --- Stage Processing Functions ---

def _run_spacy_stage(book_stem: str, stage_num: int, spacy_model: Any, args: argparse.Namespace, llm_output_base_dir: Path, processing_logic: callable) -> bool:
    Helper.logger.info(f"      Executing Stage {stage_num} for '{book_stem}'...")
    stage_output_dir = llm_output_base_dir / f"stage{stage_num}"
    output_path = stage_output_dir / f"{book_stem}.stage{stage_num}.json"
    
    if not args.force_book and Helper.is_stage_complete(book_stem, stage_num, llm_output_base_dir):
        Helper.logger.info(f"      Stage {stage_num} is already complete. Skipping."); return True

    input_path = Helper.get_input_path_for_stage(book_stem, stage_num, None, llm_output_base_dir)
    if not input_path.exists():
        Helper.logger.error(f"      Halting: Required input for Stage {stage_num} not found at {input_path}"); return False
    
    Helper.logger.info(f"      Processing file: {input_path.name}")
    try:
        with open(input_path, 'r', encoding='utf-8') as f: data = json.load(f)
    except (IOError, json.JSONDecodeError) as e:
        Helper.logger.error(f"      Could not read or parse {input_path.name}: {e}"); return False
    
    data = processing_logic(data, spacy_model, stage_num)

    if stage_num in [3, 5]: data["processing_status"] = "PARTIAL"
    else: data["processing_status"] = "COMPLETED"
    data["processing_timestamp"] = Helper.get_iso_timestamp()
    
    try:
        stage_output_dir.mkdir(parents=True, exist_ok=True)
        with open(output_path, 'w', encoding='utf-8') as f: json.dump(data, f, indent=2, ensure_ascii=False)
        Helper.logger.info(f"      Successfully wrote Stage {stage_num} output to {output_path.name}")
        return True
    except IOError as e:
        Helper.logger.error(f"      Could not write output to {output_path.name}: {e}"); return False
def _handle_llm_ssb_for_sentence(system_prompt: str, user_prompt: str, expected_ids: List[str], llm_client: Any, args: argparse.Namespace, stage_num_str: str, book_stem: str, sentence_id: str, in_log_path: Path, out_log_path: Path, require_content: bool = False) -> Optional[Dict[str, str]]:
    """Universal handler for a single sentence's batch of phrases. Includes full retry/validation logic."""
    last_error_reason, last_prompt_sent, llm_response_text = "Unknown error", user_prompt, ""
    
    for model_tier in ["primary", "fallback"]:
        model_to_use = args.llm_model if model_tier == "primary" else args.llm_fallback_model
        if not model_to_use:
            if model_tier == "fallback": Helper.logger.info("        No fallback model configured. Primary model failed permanently.")
            continue
        
        Helper.logger.info(f"        Processing sentence with {model_tier} model: '{model_to_use}'")

        for val_attempt in range(args.max_validation_retries):
            current_user_prompt = user_prompt
            if val_attempt > 0:
                corrective_header = f"PREVIOUS ATTEMPT FAILED VALIDATION. Error: '{last_error_reason}'. PLEASE REGENERATE THE FULL RESPONSE for all IDs, paying close attention to the required format and ensuring all IDs are present.\n---\n"
                current_user_prompt = corrective_header + user_prompt
                Helper.logger.warning(f"          Validation attempt {val_attempt + 1}/{args.max_validation_retries} for {sentence_id}. Reason: {last_error_reason}")

            for api_attempt in range(args.max_api_retries):
                response, error_msg = Helper._make_llm_api_call(llm_client, args.llm_provider, system_prompt, current_user_prompt, model_to_use, api_attempt + 1, args.max_api_retries, 4096)
                llm_response_text = response or ""
                last_prompt_sent = current_user_prompt
                
                if response is not None:
                    parsed_data, parse_errors = Helper._parse_llm_response_blocks(response, expected_ids)
                    
                    if not parse_errors and (not require_content or parsed_data):
                        try:
                            with open(in_log_path, 'a', encoding='utf-8') as f: f.write(f"--- PROMPT FOR {sentence_id} ---\n{last_prompt_sent}\n\n")
                            with open(out_log_path, 'a', encoding='utf-8') as f: f.write(f"--- RESPONSE FOR {sentence_id} ---\n{llm_response_text}\n\n")
                        except IOError as e: Helper.logger.warning(f"Could not write to .in/.out log files: {e}")
                        return parsed_data
                    else:
                        if not parse_errors and require_content and not parsed_data:
                            last_error_reason = "Parsing Error: LLM response was valid but contained no parsable data blocks."
                        else:
                            last_error_reason = f"Parsing Error: {parse_errors}"
                        break
                else:
                    last_error_reason = f"API Error: {error_msg}"
                
                if api_attempt < args.max_api_retries - 1:
                    Helper.logger.warning(f"          API call failed. Retrying in {args.retry_delay}s...")
                    time.sleep(args.retry_delay)
    
    error_output_dir = Path(args.project_config).parent / args.output_llm_subdir / "errors"
    Helper._write_error_debug_file(book_stem, stage_num_str, error_output_dir, f"Sentence S_ID: {sentence_id}", last_prompt_sent, llm_response_text, last_error_reason)
    Helper.logger.critical(f"      Sentence {sentence_id} failed permanently for Stage {stage_num_str}. Final error: {last_error_reason}")
    return None

def _run_llm_stage_ssb(book_stem: str, stage_num_str: str, llm_client: Any, args: argparse.Namespace, llm_output_base_dir: Path, logic: callable, spacy_models: Dict) -> bool:
    Helper.logger.info(f"      Executing Stage {stage_num_str} for '{book_stem}'...")
    current_stage_num = int(re.findall(r'\d+', stage_num_str)[0])
    
    stage_output_dir = llm_output_base_dir / f"stage{current_stage_num}"
    output_path = stage_output_dir / f"{book_stem}.stage{current_stage_num}.json"
    in_log_path = stage_output_dir / f"{book_stem}.stage{current_stage_num}.in"
    out_log_path = stage_output_dir / f"{book_stem}.stage{current_stage_num}.out"
    stage_output_dir.mkdir(parents=True, exist_ok=True)

    # Determine input path based on whether it's a 'b' stage (e.g., 3b) or a primary stage
    if 'b' in stage_num_str:
        input_path = output_path # A 'b' stage reads its own stage's file
    else:
        input_path = llm_output_base_dir / f"stage{current_stage_num - 1}" / f"{book_stem}.stage{current_stage_num - 1}.json"

    if not input_path.exists():
        Helper.logger.error(f"      Halting: Required input for Stage {stage_num_str} not found at {input_path}"); return False
    
    Helper.logger.info(f"      Processing file: {input_path.name}")
    try:
        with open(input_path, 'r', encoding='utf-8') as f: data = json.load(f)
    except (IOError, json.JSONDecodeError) as e:
        Helper.logger.error(f"      Could not read or parse {input_path.name}: {e}"); return False
    
    system_prompt, user_prompt_builder, data_updater = logic(args)
    if not system_prompt: return False

    content_blocks = data.get("content_blocks", [])
    
    for s_idx, block in enumerate(content_blocks):
        if block.get("block_type") != "sentence":
            continue

        # --- THIS IS THE KEY RESUMABILITY FIX ---
        # Check if this specific sentence block has already been processed for this stage.
        # This prevents re-processing on a script restart.
        if not args.force_book:
            llm_status = block.get("llm_call_status", {})
            if llm_status.get(f"stage{stage_num_str}") in ["COMPLETED_LLM", "COMPLETED_SPACY"]:
                Helper.logger.debug(f"        Skipping already completed sentence {block['original_sentence_s_id']} for stage {stage_num_str}")
                continue
        # --- END OF RESUMABILITY FIX ---

        user_prompt, expected_ids = user_prompt_builder(block, s_idx)
        if not user_prompt or not expected_ids:
            # Mark as complete even if there's nothing to process to avoid re-visiting
            block.setdefault("llm_call_status", {})[f"stage{stage_num_str}"] = "COMPLETED_LLM"
            continue
        
        Helper.logger.info(f"        Processing Stage {stage_num_str} for sentence {block['original_sentence_s_id']} ({len(expected_ids)} segments)")
        
        content_is_required = (stage_num_str == "7")
        parsed_data = _handle_llm_ssb_for_sentence(system_prompt, user_prompt, expected_ids, llm_client, args, stage_num_str, book_stem, block['original_sentence_s_id'], in_log_path, out_log_path, require_content=content_is_required)
        
        if parsed_data is None:
            Helper.logger.critical(f"FATAL error processing sentence {block['original_sentence_s_id']}. Halting processing for book '{book_stem}'.")
            data["processing_status"] = "FAILED"
            with open(output_path, 'w', encoding='utf-8') as f: json.dump(data, f, indent=2, ensure_ascii=False)
            return False
        
        data_updater(block, parsed_data, s_idx, spacy_models)
        block.setdefault("llm_call_status", {})[f"stage{stage_num_str}"] = "COMPLETED_LLM"
            
        data["processing_timestamp"] = Helper.get_iso_timestamp()
        try:
            with open(output_path, 'w', encoding='utf-8') as f: json.dump(data, f, indent=2, ensure_ascii=False)
        except IOError as e:
            Helper.logger.error(f"      CRITICAL: Could not write progress to {output_path.name}: {e}"); return False

    data["processing_status"] = "COMPLETED"
    try:
        with open(output_path, 'w', encoding='utf-8') as f: json.dump(data, f, indent=2, ensure_ascii=False)
        Helper.logger.info(f"      Successfully wrote final Stage {stage_num_str} output to {output_path.name}")
        return True
    except IOError as e:
        Helper.logger.error(f"      Could not write final output to {output_path.name}: {e}"); return False

def _stage2_logic(data: Dict, spacy_es: Any, stage_num: int) -> Dict:
    for block in data.get("content_blocks", []):
        if block.get("block_type") == "sentence":
            source_text = block.get("adv_spanish_full", {}).get("text", "")
            if source_text.strip():
                doc = spacy_es(source_text)
                lemmas = [t.lemma_.lower() for t in doc if not t.is_punct and not t.is_space and t.pos_ != "PROPN"]
                block.setdefault("adv_spanish_full", {})["lemmas"] = lemmas
            else: 
                block.setdefault("adv_spanish_full", {})["lemmas"] = []
        block.setdefault("llm_call_status", {})[f"stage{stage_num}"] = "COMPLETED_SPACY"
    return data

def _stage3a_logic(data: Dict, spacy_es: Any, stage_num: int) -> Dict:
    def get_syntactic_chunks(doc: spacy.tokens.Doc) -> List[spacy.tokens.Span]:
        split_points = set()
        for token in doc:
            if token.dep_ == "cc": split_points.add(token.i)
            if token.dep_ == "mark": split_points.add(token.i)
            if token.pos_ == "ADP" and token.head.pos_ in ["VERB", "NOUN", "PROPN"]:
                if token.i > 0 and len([t for t in doc[0:token.i] if not t.is_punct]) > 1:
                    split_points.add(token.i)
        sorted_split_points = sorted(list(split_points))
        final_chunks, start = [], 0
        for point in sorted_split_points:
            if start < point: final_chunks.append(doc[start:point])
            start = point
        if start < len(doc): final_chunks.append(doc[start:])
        return final_chunks
    def merge_short_chunks(chunks: List[spacy.tokens.Span], min_words: int = 2) -> List[str]:
        if not chunks: return []
        texts = [c.text.strip() for c in chunks]
        i = 0
        while i < len(texts) - 1:
            cleaned_chunk = ''.join(c for c in texts[i] if c.isalnum())
            if cleaned_chunk in ["y", "o", "pero", "que", "si", "cuando", "pues"]:
                texts[i+1] = f"{texts[i]} {texts[i+1]}"
                texts.pop(i)
            else: i += 1
        made_a_merge = True
        while made_a_merge:
            made_a_merge, i = False, 1
            if len(texts) <= 1: break
            while i < len(texts):
                if len(texts[i].split()) < min_words:
                    texts[i-1] = f"{texts[i-1]} {texts[i]}"
                    texts.pop(i)
                    made_a_merge = True; break
                else: i += 1
        return texts
    for block in data.get("content_blocks", []):
        if block.get("block_type") == "sentence":
            source_text = block.get("adv_spanish_full", {}).get("text", "")
            segments = []
            if source_text.strip():
                doc = spacy_es(source_text)
                final_phrases = merge_short_chunks(get_syntactic_chunks(doc))
                for i, phrase in enumerate(final_phrases):
                    segments.append({"segment_id": f"A{i+1}", "advanced_text": phrase, "simpler_text": "", "advanced_lemmas": [], "simpler_lemmas": []})
            block["adv_spanish_segments"] = segments
        block.setdefault("llm_call_status", {})["stage3a"] = "COMPLETED_SPACY"
    return data

def _stage3b_logic(args):
    def user_prompt_builder(block, s_idx):
        segments = block.get("adv_spanish_segments", [])
        if not segments: return None, None
        s_id_num = block['original_sentence_s_id'].replace('S', '')
        prompt_lines = [f"id {s_id_num}_{seg['segment_id']}: {seg['advanced_text']}" for seg in segments]
        expected_ids = [line.split(':')[0].lower() for line in prompt_lines]
        return "\n".join(prompt_lines), expected_ids
    def data_updater(block, parsed_data, s_idx, spacy_models):
        s_id_num = block['original_sentence_s_id'].replace('S', '')
        for seg in block.get("adv_spanish_segments", []):
            lookup_id = f"id {s_id_num}_{seg['segment_id']}".lower()
            seg['simpler_text'] = parsed_data.get(lookup_id, seg.get('advanced_text', ''))
    return Helper._load_prompt_template("new_stage3_simplifier_prompt.txt"), user_prompt_builder, data_updater
def _stage4_logic(data: Dict, spacy_es: Any, stage_num: int) -> Dict:
    for block in data.get("content_blocks", []):
        if block.get("block_type") == "sentence":
            all_simpler_texts, all_simpler_lemmas = [], []
            for seg in block.get("adv_spanish_segments", []):
                adv_doc = spacy_es(seg.get("advanced_text", ""))
                seg["advanced_lemmas"] = [t.lemma_.lower() for t in adv_doc if not t.is_punct and not t.is_space and t.pos_ != "PROPN"]
                simpler_doc = spacy_es(seg.get("simpler_text", ""))
                simpler_lemmas = [t.lemma_.lower() for t in simpler_doc if not t.is_punct and not t.is_space and t.pos_ != "PROPN"]
                seg["simpler_lemmas"] = simpler_lemmas
                all_simpler_texts.append(seg.get("simpler_text", ""))
                all_simpler_lemmas.extend(simpler_lemmas)
            block["simpler_adv_spanish_full"] = {"text": " ".join(all_simpler_texts), "lemmas": all_simpler_lemmas}
        block.setdefault("llm_call_status", {})[f"stage{stage_num}"] = "COMPLETED_SPACY"
    return data

def _stage5a_logic(data: Dict, spacy_en: Any, stage_num: int) -> Dict:
    def get_syntactic_chunks(doc: spacy.tokens.Doc) -> List[spacy.tokens.Span]:
        split_points = set()
        for token in doc:
            if token.dep_ == "cc": split_points.add(token.i)
            if token.dep_ == "mark": split_points.add(token.i)
            if token.pos_ == "ADP" and token.head.pos_ in ["VERB", "NOUN", "PROPN"]:
                if token.i > 0 and len([t for t in doc[0:token.i] if not t.is_punct]) > 1:
                    split_points.add(token.i)
        sorted_split_points = sorted(list(split_points))
        final_chunks, start = [], 0
        for point in sorted_split_points:
            if start < point: final_chunks.append(doc[start:point])
            start = point
        if start < len(doc): final_chunks.append(doc[start:])
        return final_chunks
    def merge_short_chunks(chunks: List[spacy.tokens.Span], min_words: int = 2) -> List[str]:
        if not chunks: return []
        texts = [c.text.strip() for c in chunks]
        i = 0
        while i < len(texts) - 1:
            cleaned_chunk = ''.join(c for c in texts[i] if c.isalnum()).lower()
            if cleaned_chunk in ["and", "or", "but", "so", "that", "if", "when", "as"]:
                texts[i+1] = f"{texts[i]} {texts[i+1]}"
                texts.pop(i)
            else: i += 1
        made_a_merge = True
        while made_a_merge:
            made_a_merge, i = False, 1
            if len(texts) <= 1: break
            while i < len(texts):
                if len(texts[i].split()) < min_words:
                    texts[i-1] = f"{texts[i-1]} {texts[i]}"
                    texts.pop(i)
                    made_a_merge = True; break
                else: i += 1
        return texts
    for block in data.get("content_blocks", []):
        if block.get("block_type") == "sentence":
            source_text = block.get("english_text", "")
            alignments, l3_segments = [], []
            if source_text.strip():
                doc = spacy_en(source_text)
                final_phrases = merge_short_chunks(get_syntactic_chunks(doc))
                for i, phrase in enumerate(final_phrases):
                    sid = f"S{i+1}"
                    alignments.append({"segment_id": sid, "simple_spanish_text": "", "english_span_text": phrase})
                    l3_segments.append({"segment_id": sid, "simple_text": ""})
            block["phrase_alignments_l3_to_english"] = alignments
            block["simple_spanish_l3_segments"] = l3_segments
        block.setdefault("llm_call_status", {})["stage5a"] = "COMPLETED_SPACY"
    return data

def _stage5b_logic(args):
    def user_prompt_builder(block, s_idx):
        alignments = block.get("phrase_alignments_l3_to_english", [])
        if not alignments: return None, None
        s_id_num = block['original_sentence_s_id'].replace('S', '')
        prompt_lines = [f"id {s_id_num}_{align['segment_id']}: {align['english_span_text']}" for align in alignments]
        expected_ids = [line.split(':')[0].lower() for line in prompt_lines]
        return "\n".join(prompt_lines), expected_ids
    def data_updater(block, parsed_data, s_idx, spacy_models):
        s_id_num = block['original_sentence_s_id'].replace('S', '')
        l3_segments, l3_full_texts = [], []
        for align in block.get("phrase_alignments_l3_to_english", []):
            lookup_id = f"id {s_id_num}_{align['segment_id']}".lower()
            spa_text = parsed_data.get(lookup_id, "")
            align['simple_spanish_text'] = spa_text
            l3_segments.append({"segment_id": align["segment_id"], "simple_text": spa_text})
            l3_full_texts.append(spa_text)
        block["simple_spanish_l3_segments"] = l3_segments
        block["simple_spanish_l3_full"] = {"text": " ".join(l3_full_texts), "lemmas": []}
    return Helper._load_prompt_template("new_stage5_translator_prompt.txt"), user_prompt_builder, data_updater
def _stage6_logic(data: Dict, spacy_es: Any, stage_num: int) -> Dict:
    for block in data.get("content_blocks", []):
        if block.get("block_type") == "sentence":
            lemmas_per_segment, all_l3_lemmas = {}, []
            for align in block.get("phrase_alignments_l3_to_english", []):
                doc = spacy_es(align.get("simple_spanish_text", ""))
                segment_lemmas = [t.lemma_.lower() for t in doc if not t.is_punct and not t.is_space and t.pos_ != "PROPN"]
                lemmas_per_segment[align["segment_id"]] = segment_lemmas
                all_l3_lemmas.extend(segment_lemmas)
            block["simple_spanish_l3_lemmas_per_segment"] = lemmas_per_segment
            if "simple_spanish_l3_full" in block:
                block["simple_spanish_l3_full"]["lemmas"] = all_l3_lemmas
        block.setdefault("llm_call_status", {})[f"stage{stage_num}"] = "COMPLETED_SPACY"
    return data
def _stage7_logic(args):
    system_prompt = Helper._load_prompt_template("new_stage7_diglot_prompt.txt")

    def user_prompt_builder(block, s_idx):
        alignments = block.get("phrase_alignments_l3_to_english", [])
        if not alignments: return None, None
        
        def clean_eng(text):
            return re.sub(r'[^\w\s-]', '', text).strip()

        s_id_num = block['original_sentence_s_id'].replace('S', '')
        prompt_lines = []
        expected_ids = []
        for align in alignments:
            english_text = align.get('english_span_text', '')
            if english_text.strip():
                unique_id = f"id {s_id_num}_{align['segment_id']}"
                prompt_lines.append(f"{unique_id}: {clean_eng(english_text)}")
                expected_ids.append(unique_id.lower())
        
        if not prompt_lines: return None, None
        return "\n".join(prompt_lines), expected_ids

    def data_updater(block, parsed_data, s_idx, spacy_models):
        final_diglot_entries = []
        s_id_num = block['original_sentence_s_id'].replace('S', '')
        spacy_es = spacy_models.get('es')
        if not spacy_es:
            Helper.logger.error("Spanish SpaCy model not available in data_updater for Stage 7.")
            block["diglot_map_entries"] = []
            return

        # Regex to reliably parse "English -> Spanish" lines
        mapping_regex = re.compile(r"^\s*([^->]+?)\s*->\s*(.+)$")

        for align in block.get("phrase_alignments_l3_to_english", []):
            lookup_id = f"id {s_id_num}_{align['segment_id']}".lower()
            llm_output_for_segment = parsed_data.get(lookup_id, "")
            
            # Step 1: Parse all valid mappings from the LLM output into a dictionary.
            # This is more robust than looping and searching for each word.
            llm_mappings = {}
            for line in llm_output_for_segment.splitlines():
                match = mapping_regex.match(line)
                if match:
                    eng_part = match.group(1).strip()
                    spa_part = match.group(2).strip()
                    # Use a case-insensitive key for the dictionary
                    llm_mappings[eng_part.lower()] = spa_part

            # Step 2: Iterate through the original English words and use the parsed map.
            original_eng_text = align.get("english_span_text", "")
            expected_eng_words = [word for word in re.sub(r'[^\w\s-]', '', original_eng_text).strip().split(' ') if word]
            
            for eng_word in expected_eng_words:
                # Perform a case-insensitive lookup in our pre-parsed map
                spa_form = llm_mappings.get(eng_word.lower(), "NO_SUB")

                note, is_viable = spa_form, True
                if spa_form in ["PROPER_NOUN", "NO_SUB"]:
                    is_viable = False
                    spa_lemma = eng_word.lower()
                else:
                    doc = spacy_es(spa_form)
                    spa_lemma = doc[0].lemma_.lower() if doc and len(doc) > 0 else spa_form.lower()
                    note = "viable"
                
                final_diglot_entries.append({
                    "segment_id": align["segment_id"], "english_word": eng_word, "spanish_lemma": spa_lemma,
                    "exact_spanish_form": spa_form, "is_viable_for_substitution": is_viable, "note": note
                })
        block["diglot_map_entries"] = final_diglot_entries

    return system_prompt, user_prompt_builder, data_updater

# --- Main Orchestration Entry Point ---
def main():
    parser = argparse.ArgumentParser(description="Orchestrates the WeaveLang multi-stage LLM & SpaCy processing pipeline.", formatter_class=argparse.ArgumentDefaultsHelpFormatter)
    parser.add_argument("--project_config", default="config.toml")
    parser.add_argument("--input_staged_subdir", default="Staged")
    parser.add_argument("--output_llm_subdir", default="stage")
    parser.add_argument("--force_book", type=str, default=None)
    parser.add_argument("--book_to_process", type=str, default=None)
    parser.add_argument("--start_at_stage", type=int, default=1, choices=range(1, Helper.MAX_STAGES + 1))
    parser.add_argument("--llm_provider", default="claude", choices=["gemini", "claude"])
    parser.add_argument("--llm_model", help="Primary LLM model name.")
    parser.add_argument("--llm_fallback_model", help="Fallback LLM model name.")
    parser.add_argument("--max_sentences_per_batch", type=int, default=5)
    parser.add_argument("--max_api_retries", type=int, default=3)
    parser.add_argument("--max_validation_retries", type=int, default=4)
    parser.add_argument("--retry_delay", type=int, default=7)
    args = parser.parse_args()

    Helper.logger.info("--- WeaveLang Pipeline Orchestrator Initializing ---")
    
    try:
        with open(args.project_config, "rb") as f: config_data = tomllib.load(f)
        content_project_root_str = config_data.get("content_project_dir")
    except Exception as e:
        Helper.logger.critical(f"Failed to load config: {e}"); sys.exit(1)
    if not content_project_root_str:
        Helper.logger.critical(f"'content_project_dir' not found in config."); sys.exit(1)

    content_project_root = Path(content_project_root_str)
    staged_dir = content_project_root / args.input_staged_subdir
    llm_output_base_dir = content_project_root / args.output_llm_subdir

    Helper.logger.info("Initializing shared resources...")
    llm_client = Helper.initialize_llm_client(args)
    if llm_client is None: sys.exit(1)
    
    spacy_models = {}
    try:
        Helper.logger.info("Loading SpaCy models...")
        spacy_models['en'] = spacy.load("en_core_web_lg", disable=["ner"])
        spacy_models['es'] = spacy.load("es_core_news_lg", disable=["ner"])
        Helper.logger.info("SpaCy models loaded successfully.")
    except IOError:
        Helper.logger.critical("SpaCy model not found. Run `python -m spacy download en_core_web_lg` and `... es_core_news_lg`"); sys.exit(1)

    book_stems = [args.book_to_process] if args.book_to_process else sorted([f.stem for f in staged_dir.glob('*.txt') if not f.name.endswith('.junk.txt')])
    if not book_stems: Helper.logger.info("No books found to process."); return

    Helper.logger.info(f"Orchestrator starting. Found {len(book_stems)} book(s) to process: {book_stems}")

    for book_stem in book_stems:
        Helper.logger.info(f"--- Starting Pipeline for Book: [{book_stem}] ---")
        
        effective_start_stage = 1
        if args.force_book == book_stem:
            effective_start_stage = args.start_at_stage
            Helper.logger.warning(f"Force reprocessing '{book_stem}' starting from Stage {effective_start_stage}.")
        else:
            for i in range(Helper.MAX_STAGES, 0, -1):
                if Helper.is_stage_complete(book_stem, i, llm_output_base_dir):
                    effective_start_stage = i + 1; break
        
        if effective_start_stage > Helper.MAX_STAGES:
            Helper.logger.info(f"Book '{book_stem}' is already fully processed. Skipping."); continue
            
        Helper.logger.info(f"Effective start stage for '{book_stem}' is Stage {effective_start_stage}.")
        
        pipeline_ok = True
        for stage_to_run in [1]: #range(effective_start_stage, Helper.MAX_STAGES + 1):
            if not pipeline_ok:
                Helper.logger.error(f"Halting pipeline for '{book_stem}' due to previous stage failure."); break

            if stage_to_run == 1:
                Stage1(book_stem, llm_client, args, staged_dir, llm_output_base_dir)
                pipeline_ok = Stage1.run() 
            elif stage_to_run == 2:
                pipeline_ok = _run_spacy_stage(book_stem, 2, spacy_models['es'], args, llm_output_base_dir, _stage2_logic)
            elif stage_to_run == 3:
                pipeline_ok = _run_spacy_stage(book_stem, 3, spacy_models['es'], args, llm_output_base_dir, _stage3a_logic)
                if pipeline_ok: pipeline_ok = _run_llm_stage_ssb(book_stem, "3b", llm_client, args, llm_output_base_dir, _stage3b_logic, spacy_models)
            elif stage_to_run == 4:
                pipeline_ok = _run_spacy_stage(book_stem, 4, spacy_models['es'], args, llm_output_base_dir, _stage4_logic)
            elif stage_to_run == 5:
                pipeline_ok = _run_spacy_stage(book_stem, 5, spacy_models['en'], args, llm_output_base_dir, _stage5a_logic)
                if pipeline_ok: pipeline_ok = _run_llm_stage_ssb(book_stem, "5b", llm_client, args, llm_output_base_dir, _stage5b_logic, spacy_models)
            elif stage_to_run == 6:
                pipeline_ok = _run_spacy_stage(book_stem, 6, spacy_models['es'], args, llm_output_base_dir, _stage6_logic)
            elif stage_to_run == 7:
                pipeline_ok = _run_llm_stage_ssb(book_stem, "7", llm_client, args, llm_output_base_dir, _stage7_logic, spacy_models)

        if pipeline_ok: Helper.logger.info(f"--- Successfully Finished Pipeline for Book: [{book_stem}] ---\n")
        else: Helper.logger.error(f"--- Pipeline FAILED for Book: [{book_stem}]. See logs for details. ---\n")

    Helper.logger.info("All books have been processed by the orchestrator.")

if __name__ == "__main__":
    main()