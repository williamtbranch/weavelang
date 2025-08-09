# llm2books/orchestrate_pipeline.py

import argparse
import logging
import sys
from pathlib import Path
import re
from typing import Optional, Dict, Any
from .stanza_segmenter import EnglishStanzaProcessor

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
    # The --base-lang and --target-lang arguments are now deprecated from the orchestrator
    # as this is determined by the %%lang:xx%% tag in the source file.
    # We will leave them for now to avoid breaking any scripts, but they are unused.
    parser.add_argument("--base-lang", help="[DEPRECATED] Base language is now detected from source file.")
    parser.add_argument("--target-lang", default="es", help="Target language code (e.g., 'es').")
    parser.add_argument("--book-to-process", type=str, default=None, help="Process only a single specified book stem.")
    parser.add_argument("--force_book", type=str, default=None, help="Force reprocessing of a specific book, ignoring existing progress.")
    parser.add_argument("--start_at_stage", type=int, default=None, help="Start processing from a specific stage number.")

    args = parser.parse_args()
    
    logger.info("--- WeaveLang Pipeline Orchestrator Initializing ---")

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
    except Exception as e:
        logger.critical(f"Failed to load language manifest: {e}")
        sys.exit(1)

    # --- Book Discovery ---
    staged_dir = content_project_root / "Staged"
    if args.book_to_process:
        source_files_to_process = [staged_dir / f"{args.book_to_process}.txt"]
        if not source_files_to_process[0].exists():
             logger.critical(f"Specified book not found: {source_files_to_process[0]}")
             sys.exit(1)
    else:
        source_files_to_process = sorted(staged_dir.glob("*.txt"))

    if not source_files_to_process:
        logger.warning(f"No source .txt files found to process in: {staged_dir}")
        return

    logger.info(f"Found {len(source_files_to_process)} potential source file(s) in Staged directory.")
    
    # --- Initialize Shared Resources (SpaCy, Stanza, LLM Client) ---
    # These are expensive, so we load them once for the entire run.
    logger.info("Initializing shared resources (this may take a moment)...")
    
    # Determine all languages needed for this run
    needed_langs = {args.target_lang}
    for book_path in source_files_to_process:
        source_lang = get_source_lang_from_file(book_path)
        if source_lang:
            needed_langs.add(source_lang)
    
    spacy_models: Dict[str, Any] = {}
    stanza_processors: Dict[str, Any] = {}
    for lang_code in needed_langs:
        lang_info = lang_manifest.get(lang_code)
        if not lang_info:
            logger.warning(f"Language '{lang_code}' found in a source file, but not defined in languages.toml. Skipping.")
            continue
        # Load SpaCy model
        try:
            spacy_model_name = lang_info.get("spacy_model")
            if spacy_model_name:
                logger.info(f"Loading SpaCy model for '{lang_code}': {spacy_model_name}")
                spacy_models[lang_code] = spacy.load(spacy_model_name, disable=["ner"])
        except IOError as e:
            logger.critical(f"SpaCy model for '{lang_code}' not found. Please run 'python -m spacy download {spacy_model_name}'. Error: {e}")
            sys.exit(1)
        # Load Stanza model
        if lang_code == 'en': # Extend this with 'es' etc. when SpanishStanzaProcessor is created
            logger.info(f"Loading Stanza processor for '{lang_code}'...")
            stanza_processors[lang_code] = EnglishStanzaProcessor()

    llm_provider = pipeline_config.get("llm_provider", "claude")
    llm_client = helper.initialize_llm_client(llm_provider)
    if llm_client is None:
        sys.exit(1)

    # --- Main Processing Loop ---
    overall_success = True
    for book_path in source_files_to_process:
        book_stem = book_path.stem
        source_lang = get_source_lang_from_file(book_path)
        
        if not source_lang:
            logger.warning(f"Skipping '{book_path.name}': Missing or malformed %%lang:xx%% tag on the first line.")
            continue
        
        try:
            language_config = build_language_config(lang_manifest, source_lang, args.target_lang)
        except ValueError as e:
            logger.error(f"Skipping '{book_path.name}': {e}")
            continue

        logger.info(f"--- Starting Pipeline for Book: [{book_stem}] ({source_lang} -> {args.target_lang}) ---")
        
        # Assemble book-specific common resources for this run
        common_resources = {
            'llm_client': llm_client,
            'spacy_models': spacy_models,
            'stanza_processors': stanza_processors,
            'content_project_dir': content_project_dir_str,
            'tool_root_dir': tool_root_dir,
            'models_config': models_config,
            'pipeline_config': pipeline_config,
            'stages_config': stages_config,
            'language_config': language_config, # This is now specific to the book's language pair
            'source_path': book_path, # Pass the source path down
        }

        pipeline_ok = True
        for StageClass in PIPELINE_STAGES:
            stage_instance = StageClass(book_stem, args, common_resources)
            
            # Logic to skip stages if --start_at_stage is used
            if args.start_at_stage and stage_instance.stage_number < args.start_at_stage:
                logger.info(f"Skipping Stage {stage_instance.stage_number} ({stage_instance.stage_name}) as requested.")
                continue

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