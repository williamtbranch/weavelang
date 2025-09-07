# assets/prompt_testing/rate_prompts.py
import argparse
import json
import sys
import os
import time
from pathlib import Path
import re
import unicodedata
from collections import Counter
from tqdm import tqdm
import numpy as np

import spacy

try:
    from anthropic import Anthropic
    from dotenv import load_dotenv
except ImportError:
    print("CRITICAL: 'anthropic' and 'python-dotenv' libraries are required.", file=sys.stderr)
    sys.exit(1)

# ... (run_llm_simplification_batch, normalize_spanish_lemma, load_frequency_map, score_prompt_results are unchanged) ...
def run_llm_simplification_batch(
    client: Anthropic, prompt_text: str, segments: dict,
    model_name: str = "claude-sonnet-4-20250514", batch_size: int = 20,
    max_retries: int = 3, retry_delay: int = 7
) -> dict:
    simplified_segments = {}
    segment_items = list(segments.items())
    for i in tqdm(range(0, len(segment_items), batch_size), desc="  -> Simplifying Batches"):
        batch = segment_items[i:i + batch_size]
        user_prompt_content = "\n".join([f"{seg_id}: {text}" for seg_id, text in batch])
        for attempt in range(max_retries):
            try:
                message = client.messages.create(
                    model=model_name, max_tokens=4096, system=prompt_text,
                    messages=[{"role": "user", "content": user_prompt_content}]
                )
                response_text = message.content[0].text
                for line in response_text.splitlines():
                    if ":" in line:
                        parts = line.split(":", 1)
                        seg_id, text = parts[0].strip(), parts[1].strip()
                        if seg_id in segments: simplified_segments[seg_id] = text
                if all(seg_id in simplified_segments for seg_id, _ in batch):
                    break
                else:
                    missing_ids = [seg_id for seg_id, _ in batch if seg_id not in simplified_segments]
                    print(f"\n      [WARNING] Batch {i//batch_size+1}: Missing {len(missing_ids)} IDs (e.g., {missing_ids[0]}). Retrying ({attempt+1}/{max_retries})...")
                    time.sleep(retry_delay)
            except Exception as e:
                print(f"\n      [ERROR] API call failed on attempt {attempt+1}/{max_retries}: {e}")
                if attempt < max_retries - 1:
                    time.sleep(retry_delay * (attempt + 1))
                else:
                    print("      [FATAL] Max retries reached. Using original text for failed segments in this batch.")
                    for seg_id, text in batch:
                        if seg_id not in simplified_segments: simplified_segments[seg_id] = text
                    break
    for seg_id, original_text in segments.items():
        if seg_id not in simplified_segments: simplified_segments[seg_id] = original_text
    return simplified_segments

def normalize_spanish_lemma(lemma_str: str) -> str:
    s = lemma_str.lower().strip().split(' ')[0]
    s = (s.replace('á', 'a').replace('é', 'e').replace('í', 'i').replace('ó', 'o').replace('ú', 'u').replace('ñ', 'n').replace('ü', 'u'))
    s = re.sub(r'^[^\w]+|[^\w]+$', '', s)
    if not s: return ""
    s = unicodedata.normalize('NFC', s)
    if re.search(r'[^a-z-]', s): return ""
    return s

def load_frequency_map(freq_list_path: Path) -> dict[str, int]:
    print(f"Loading frequency map from: {freq_list_path}")
    if not freq_list_path.is_file(): print(f"ERROR: Freq list not found at '{freq_list_path}'", file=sys.stderr); sys.exit(1)
    lemma_to_rank = {}
    with open(freq_list_path, "r", encoding="utf-8") as f:
        next(f)
        for line in f:
            parts = line.strip().split("\t")
            if len(parts) >= 2:
                try: lemma_to_rank[parts[0]] = int(parts[1])
                except ValueError: continue
    print(f"Successfully loaded {len(lemma_to_rank):,} lemmas.")
    return lemma_to_rank

def score_prompt_results(
    results: dict,
    nlp_model,
    freq_map: dict[str, int],
    max_rank: int
) -> dict:
    """
    Lemmatizes text and calculates a detailed breakdown of vocabulary rank scores
    using a corrected percentile calculation method.
    """
    all_lemmas = []
    text_to_process = list(results.values())
    
    doc_iterator = nlp_model.pipe(text_to_process, batch_size=50)
    for doc in tqdm(doc_iterator, total=len(text_to_process), desc="  -> Lemmatizing"):
        for token in doc:
            if not token.is_punct and not token.is_space and token.pos_ != "PROPN":
                normalized_lemma = normalize_spanish_lemma(token.lemma_)
                if normalized_lemma:
                    all_lemmas.append(normalized_lemma)
    
    if not all_lemmas:
        return {
            "avg_rank": 0.0, "tail_score": 0.0, "p85_rank": 0, "p95_rank": 0,
            "p98_rank": 0, "p99_rank": 0, "max_rank": 0
        }

    # --- NEW, CORRECT LOGIC ---
    # 1. Create a list of the global rank for every single lemma instance.
    all_ranks = [freq_map.get(lemma, max_rank) for lemma in all_lemmas]

    # 2. Use numpy.percentile for a fast and accurate calculation.
    #    This is the standard, robust way to do this in Python.
    p85_rank = int(np.percentile(all_ranks, 85))
    p95_rank = int(np.percentile(all_ranks, 95))
    p98_rank = int(np.percentile(all_ranks, 98))
    p99_rank = int(np.percentile(all_ranks, 99))
    max_rank_in_text = int(np.max(all_ranks)) if all_ranks else 0
    
    # --- Final Score Calculations ---
    simple_avg_rank = np.mean(all_ranks)
    tail_weighted_score = (p85_rank + (p95_rank * 2)) / 3.0

    return {
        "avg_rank": simple_avg_rank,
        "tail_score": tail_weighted_score,
        "p85_rank": p85_rank,
        "p95_rank": p95_rank,
        "p98_rank": p98_rank,
        "p99_rank": p99_rank,
        "max_rank": max_rank_in_text
    }


# --- MAIN SCRIPT ---
def main():
    parser = argparse.ArgumentParser(description="Rate WeaveLang prompts based on vocabulary frequency.")
    parser.add_argument("--freq-list", type=Path, default=Path("assets/frequency_lists/es_master_frequency_list.txt"))
    args = parser.parse_args()
    print("--- WeaveLang Prompt Rater ---")

    # Configuration
    TEST_BOOKS_DIR = Path("E:/Bill/Documents/development/audiolingual/test_books")
    PROMPT_VARIATIONS_DIR = Path("./assets/prompt_testing/prompt_variations")
    LLM_CACHE_DIR = Path("./assets/prompt_testing/llm_response_cache")
    PROMPT_VARIATIONS_DIR.mkdir(exist_ok=True)
    LLM_CACHE_DIR.mkdir(exist_ok=True)

    # Load shared resources
    freq_map = load_frequency_map(args.freq_list)
    max_rank = len(freq_map) + 1

    # --- UPDATED: Input Caching Logic for SEGMENTS ---
    input_cache_path = LLM_CACHE_DIR / "_input_segments_cache.json"
    advanced_segments = {}

    if input_cache_path.exists():
        print(f"Loading cached input segments from: {input_cache_path.name}")
        with open(input_cache_path, "r", encoding="utf-8") as f:
            advanced_segments = json.load(f)
    else:
        print(f"No input cache found. Generating from book files in: {TEST_BOOKS_DIR}")
        json_files = sorted(list(TEST_BOOKS_DIR.glob("*.json")))
        for json_path in json_files:
            with open(json_path, "r", encoding="utf-8") as f: data = json.load(f)
            for block in data.get("content_blocks", []):
                if block.get("block_type") == "sentence":
                    s_id = block.get("s_id")
                    if not s_id: continue
                    for tier in block.get("tiers", []):
                        if tier.get("tier_id") == "advanced_target":
                            # Iterate through segments in the advanced tier
                            for segment in tier.get("segments", []):
                                seg_id = segment.get("seg_id")
                                seg_text = segment.get("text", "")
                                if seg_id and seg_text.strip():
                                    # Create a unique key for each segment
                                    unique_key = f"{s_id}_{seg_id}"
                                    advanced_segments[unique_key] = seg_text
                            break # Move to the next sentence block
        
        print(f"Saving {len(advanced_segments)} input segments to cache: {input_cache_path.name}")
        with open(input_cache_path, "w", encoding="utf-8") as f:
            json.dump(advanced_segments, f, indent=2, ensure_ascii=False)
    
    print(f"Loaded {len(advanced_segments)} ADVANCED segments to use as LLM input.\n")
    if not advanced_segments: print("No advanced segments found.", file=sys.stderr); sys.exit(1)

    # Load NLP and LLM resources
    print("Loading SpaCy model (es_core_news_lg)...")
    nlp = spacy.load("es_core_news_lg")
    print("SpaCy model loaded.")
    dotenv_path = Path("./.env"); load_dotenv(dotenv_path=dotenv_path)
    api_key = os.getenv("ANTHROPIC_API_KEY")
    if not api_key: print("CRITICAL: ANTHROPIC_API_KEY not found.", file=sys.stderr); sys.exit(1)
    llm_client = Anthropic(api_key=api_key)
    print("Anthropic LLM client initialized.\n")

    # Main Processing Loop
    all_scores = {}
    
    # --- NEW: Calculate Baseline Score for Advanced Text ---
    print("--- Calculating Baseline Score for Original Advanced Text ---")
    all_scores["_Baseline (Advanced Text)"] = score_prompt_results(advanced_segments, nlp, freq_map, max_rank)
    print(f"  -> Simple Avg. Rank: {all_scores['_Baseline (Advanced Text)']['avg_rank']:.2f}")
    print(f"  -> Tail-Weighted Score: {all_scores['_Baseline (Advanced Text)']['tail_score']:.2f}\n")
    # --- END NEW ---

    prompt_files = sorted(PROMPT_VARIATIONS_DIR.glob("simplify_*.txt"))
    if not prompt_files:
        print(f"No prompt files matching 'simplify_*.txt' found in '{PROMPT_VARIATIONS_DIR}'.", file=sys.stderr); sys.exit(1)

    for prompt_path in prompt_files:
        prompt_name = prompt_path.stem
        print(f"--- Processing Prompt: {prompt_name} ---")
        output_cache_path = LLM_CACHE_DIR / f"{prompt_name}_results.json"
        
        simplified_results_for_scoring = {}
        if output_cache_path.exists():
            print(f"  -> Found cached LLM results. Loading from {output_cache_path.name}")
            with open(output_cache_path, "r", encoding="utf-8") as f:
                cached_data = json.load(f)
                for item in cached_data: simplified_results_for_scoring[item["seg_id"]] = item["output_text"]
        else:
            print("  -> No cache found. Running LLM simplification...")
            prompt_text = prompt_path.read_text(encoding="utf-8")
            llm_output_dict = run_llm_simplification_batch(llm_client, prompt_text, advanced_segments)
            
            output_cache_data = []
            for seg_id, input_text in advanced_segments.items():
                output_text = llm_output_dict.get(seg_id, "")
                output_cache_data.append({"seg_id": seg_id, "input_text": input_text, "output_text": output_text})
                simplified_results_for_scoring[seg_id] = output_text
            
            print(f"  -> Caching LLM results to {output_cache_path.name}")
            with open(output_cache_path, "w", encoding="utf-8") as f: json.dump(output_cache_data, f, indent=2, ensure_ascii=False)

        print("  -> Scoring results...")
        scores = score_prompt_results(simplified_results_for_scoring, nlp, freq_map, max_rank)
        all_scores[prompt_path.name] = scores
        print(f"  -> Simple Avg. Rank: {scores['avg_rank']:.2f}")
        print(f"  -> Tail-Weighted Score: {scores['tail_score']:.2f}\n")

    # Final Report
    print("\n--- Prompt Simplification Final Report ---")
    print("(Lower Score is Better. Ranks shown are from the master frequency list.)")
    # A wider format for the new data
    print("-" * 120)
    header = (
        f"{'Prompt Name':<32} | {'Avg Rank':<10} | {'Tail Score':<12} | "
        f"{'P85 Rank':<10} | {'P95 Rank':<10} | {'P98 Rank':<10} | {'P99 Rank':<10} | {'Max Rank':<10}"
    )
    print(header)
    print("-" * 120)
    
    # Sort by tail score, but handle the baseline entry specially to keep it at the top
    sorted_scores = sorted(all_scores.items(), key=lambda item: (item[0] == '_Baseline (Advanced Text)', item[1]['tail_score']))
    
    for name, scores in sorted_scores:
        # Pad the name if it's the baseline for alignment
        display_name = name if name != '_Baseline (Advanced Text)' else '_Baseline'
        
        # Format all score strings
        avg_r = f"{scores['avg_rank']:.2f}"
        tail_s = f"{scores['tail_score']:.2f}"
        p85_r = str(scores['p85_rank'])
        p95_r = str(scores['p95_rank'])
        p98_r = str(scores['p98_rank'])
        p99_r = str(scores['p99_rank'])
        max_r = str(scores['max_rank'])

        row = (
            f"{display_name:<32} | {avg_r:<10} | {tail_s:<12} | "
            f"{p85_r:<10} | {p95_r:<10} | {p98_r:<10} | {p99_r:<10} | {max_r:<10}"
        )
        print(row)
        
    print("-" * 120)

if __name__ == "__main__":
    main()