# llm2books/orchestrate_pipeline.py

import argparse
import logging
import sys
from pathlib import Path
import re
from typing import Optional, Dict, Any

# --- Dependency Imports ---
try:
    import tomllib
except ImportError:
    try:
        import toml as tomllib
    except ImportError:
        print("CRITICAL: 'toml' library not found. Please run `pip install toml` for Python < 3.11.", file=sys.stderr)
        sys.exit(1)
try:
    import spacy
except ImportError:
    print("CRITICAL: SpaCy library not found. Please run `pip install spacy`.", file=sys.stderr)
    sys.exit(1)

# --- Local Module Imports ---
from . import helper
from .stages.initialize_book_tiers import InitializeBookTiers
from .stages.segment_core_tiers import SegmentCoreTiers
# from .stages.segment_core_tiers import SegmentCoreTiers # Placeholder for Stage 2
# ... etc.

# --- The New 6-Stage Pipeline Flow ---
# We will uncomment these as we create and test each new stage class.
PIPELINE_STAGES = [
    InitializeBookTiers,      # Stage 1
    SegmentCoreTiers,         # Stage 2
    # GenerateDerivedTiers,   # Stage 3
    # GenerateMappings,       # Stage 4
    # TokenizeAndLemmatizeSimplerTarget, # Stage 5
    # FinalizeAnnotations,    # Stage 6
]

# --- Logging Setup ---
def get_logger() -> logging.Logger:
    logger = logging.getLogger("pipeline")
    if logger.hasHandlers():
        return logger

    log_formatter = logging.Formatter(
        "%(asctime)s - %(levelname)s - %(name)s - %(message)s"
    )
    logger.setLevel(logging.INFO)

    console_handler = logging.StreamHandler(sys.stdout)
    console_handler.setLevel(logging.INFO)
    console_handler.setFormatter(log_formatter)
    logger.addHandler(console_handler)

    return logger

# --- Configuration Logic ---
def build_language_config(manifest: dict, base_lang: str, target_lang: str) -> dict:
    base_conf = manifest.get(base_lang)
    target_conf = manifest.get(target_lang)

    if not base_conf or not target_conf:
        missing = "base" if not base_conf else "target"
        code = base_lang if not base_conf else target_lang
        raise ValueError(f"Configuration for {missing} language '{code}' not found in language manifest.")

    lang_pair_key = f"{base_lang}-{target_lang}"
    pair_conf = manifest.get("pair", {}).get(lang_pair_key, {})
    pair_prompt_dir = pair_conf.get("prompt_dir")

    return {
        "base_code": base_lang,
        "target_code": target_lang,
        "base_name": base_conf.get("name"),
        "target_name": target_conf.get("name"),
        "base_spacy_model": base_conf.get("spacy_model"),
        "target_spacy_model": target_conf.get("spacy_model"),
        "pair_prompt_dir": pair_prompt_dir,
        "manifest": manifest,
    }

def get_source_lang_from_file(file_path: Path) -> Optional[str]:
    """Reads the %%lang:xx%% tag from the first line of a file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            first_line = f.readline()
        
        match = re.match(r"^%%lang:(\w+)%%$", first_line.strip())
        if match:
            return match.group(1)
    except Exception:
        return None
    return None

# --- Main Orchestration Entry Point ---
def main():
    logger = get_logger()
    
    parser = argparse.ArgumentParser(
        description="Orchestrates the WeaveLang data generation pipeline.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter
    )
    
    # --- Argument Parsing ---
    parser.add_argument("--project_config", default="config.toml", help="Path to the main project TOML configuration file.")
    parser.add_argument("--base-lang", required=True, help="Base language code (e.g., 'en')")
    parser.add_argument("--target-lang", required=True, help="Target language code (e.g., 'es')")
    parser.add_argument("--book-to-process", type=str, default=None, help="Process only a single specified book stem.")
    # Add other control args like force_book, start_at_stage as needed.
    
    args = parser.parse_args()
    
    logger.info("--- WeaveLang Pipeline Orchestrator Initializing ---")
    logger.info(f"Run configured for: {args.base_lang} -> {args.target_lang}")

    # --- Load Project Config (config.toml) ---
    try:
        with open(args.project_config, "rb") as f:
            config = tomllib.load(f)
        content_project_dir_str = config.get("content_project_dir")
        pipeline_config = config.get("pipeline", {})
        models_config = config.get("models", {})
        stages_config = config.get("stages", {})
        if not content_project_dir_str:
            raise ValueError("'content_project_dir' not found in config.")
    except Exception as e:
        logger.critical(f"Failed to load or parse config file '{args.project_config}': {e}")
        sys.exit(1)

    # --- Load Tool-Specific Configs (languages.toml) ---
    tool_root_dir = Path(__file__).resolve().parent.parent
    content_project_root = Path(content_project_dir_str)
    
    try:
        with open(tool_root_dir / "assets" / "languages.toml", "rb") as f:
            lang_manifest = tomllib.load(f)
        language_config = build_language_config(lang_manifest, args.base_lang, args.target_lang)
    except Exception as e:
        logger.critical(f"Failed to load or process language manifest: {e}")
        sys.exit(1)

    # --- Initialize Shared Resources (SpaCy, LLM Client) ---
    logger.info("Initializing shared resources...")
    spacy_models: Dict[str, Any] = {}
    try:
        logger.info(f"Loading SpaCy model for base language '{args.base_lang}': {language_config['base_spacy_model']}")
        spacy_models[args.base_lang] = spacy.load(language_config["base_spacy_model"], disable=["ner"])
        logger.info(f"Loading SpaCy model for target language '{args.target_lang}': {language_config['target_spacy_model']}")
        spacy_models[args.target_lang] = spacy.load(language_config["target_spacy_model"], disable=["ner"])
    except IOError as e:
        logger.critical(f"SpaCy model not found. Please run 'python -m spacy download <model_name>'. Error: {e}")
        sys.exit(1)

    llm_provider = "claude" # Default, can be made dynamic later
    llm_client = helper.initialize_llm_client(llm_provider)
    if llm_client is None:
        sys.exit(1)
        
    # --- Assemble final common_resources dictionary ---
    common_resources = {
        'llm_client': llm_client,
        'spacy_models': spacy_models,
        'content_project_dir': content_project_dir_str,
        'tool_root_dir': tool_root_dir,
        'models_config': models_config,
        'pipeline_config': pipeline_config,
        'stages_config': stages_config,
        'language_config': language_config,
    }

    # --- Book Discovery ---
    staged_dir = content_project_root / "Staged"
    if args.book_to_process:
        book_files_to_process = [staged_dir / f"{args.book_to_process}.txt"]
    else:
        book_files_to_process = sorted(staged_dir.glob("*.txt"))

    if not any(p.is_file() for p in book_files_to_process):
        logger.warning(f"No books found to process in: {staged_dir}")
        return

    logger.info(f"Found {len(book_files_to_process)} potential source file(s) in Staged directory.")
    
    # --- Main Processing Loop ---
    overall_success = True
    for book_path in book_files_to_process:
        if not book_path.is_file(): continue # Skip directories
        
        book_stem = book_path.stem
        source_lang = get_source_lang_from_file(book_path)
        
        if not source_lang:
            logger.warning(f"Skipping '{book_path.name}': Missing or malformed %%lang:xx%% tag on the first line.")
            continue
        
        logger.info(f"--- Starting Pipeline for Book: [{book_stem}] (Source Language: {source_lang}) ---")
        
        # Add book-specific info to a copy of resources for this specific run
        run_resources = common_resources.copy()
        run_resources['source_lang'] = source_lang
        run_resources['source_path'] = book_path

        pipeline_ok = True
        for StageClass in PIPELINE_STAGES:
            stage_instance = StageClass(book_stem, args, run_resources)
            
            pipeline_ok = stage_instance.run()

            if not pipeline_ok:
                logger.error(f"Halting pipeline for '{book_stem}' due to failure in stage: {stage_instance.stage_name}.")
                overall_success = False
                break
        
        if pipeline_ok:
            logger.info(f"--- Successfully Finished Pipeline for Book: [{book_stem}] ---\n")
        else:
            logger.error(f"--- Pipeline FAILED for Book: [{book_stem}]. See logs for details. ---\n")

    if overall_success:
        logger.info("Orchestrator finished successfully for all processed books.")
        sys.exit(0)
    else:
        logger.error("Orchestrator finished, but one or more books failed processing.")
        sys.exit(1)

if __name__ == "__main__":
    main()