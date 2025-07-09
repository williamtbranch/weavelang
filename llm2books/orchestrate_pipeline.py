# Filename: llm2books/orchestrate_pipeline.py
# Purpose: Main entry point for the WeaveLang data generation pipeline.
# Description: This script uses a class-based, stage-driven architecture to
# process books. It initializes shared resources and then runs a sequence of
# Stage objects, each responsible for one part of the pipeline.

import argparse
import logging
import sys
from pathlib import Path

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
# Import all the concrete stage classes
from .stages.generate_adv_spanish import GenerateAdvSpanish
from .stages.lemmatize_adv_spanish import LemmatizeAdvSpanish
from .stages.segment_adv_spanish import SegmentAdvSpanish
from .stages.simplify_adv_spanish import SimplifyAdvSpanish
from .stages.finalize_simpler_spanish import FinalizeSimplerSpanish
from .stages.segment_english import SegmentEnglish
from .stages.generate_simple_spanish import GenerateSimpleSpanish
from .stages.lemmatize_simple_spanish import LemmatizeSimpleSpanish
from .stages.generate_diglot_map import GenerateDiglotMap
from .stages.lemmatize_diglot_map import LemmatizeDiglotMap
# --- NEW IMPORTS FOR NEW STAGES ---
from .stages.generate_inverse_diglot_map import GenerateInverseDiglotMap
from .stages.lemmatize_inverse_diglot_map import LemmatizeInverseDiglotMap


# --- The Pipeline Stage Registry ---
PIPELINE_STAGES = [
    GenerateAdvSpanish,          # Stage 1
    LemmatizeAdvSpanish,         # Stage 2
    SegmentAdvSpanish,           # Stage 3a
    SimplifyAdvSpanish,          # Stage 3b
    FinalizeSimplerSpanish,      # Stage 4
    SegmentEnglish,              # Stage 5a
    GenerateSimpleSpanish,       # Stage 5b
    LemmatizeSimpleSpanish,      # Stage 6
    GenerateDiglotMap,           # Stage 7
    LemmatizeDiglotMap,          # Stage 8
    # --- APPENDED NEW STAGES ---
    GenerateInverseDiglotMap,    # Stage 9 (NEW)
    LemmatizeInverseDiglotMap,   # Stage 10 (NEW)
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

# --- Main Orchestration Entry Point ---
def main():
    logger = get_logger()
    
    parser = argparse.ArgumentParser(
        description="Orchestrates the WeaveLang multi-stage processing pipeline.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    
    # --- Argument Parsing ---
    parser.add_argument("--project_config", default="config.toml", help="Path to the main TOML configuration file.")
    parser.add_argument("--version", default="7.1.0-inverse-diglot", help="Pipeline version for metadata.")
    parser.add_argument("--input_staged_subdir", default="Staged", help="Subdirectory for initial staged text files.")
    parser.add_argument("--output_llm_subdir", default="pipeline", help="Base subdirectory for intermediate pipeline stage outputs.")
    # --- Execution control arguments ---
    parser.add_argument("--force_book", type=str, default=None, help="Force reprocessing of a specific book, ignoring existing progress.")
    parser.add_argument("--book_to_process", type=str, default=None, help="Process only a single specified book.")
    parser.add_argument(
        "--start_at_stage", type=int, default=1, choices=range(1, len(PIPELINE_STAGES) + 2), help="Start processing from a specific stage number."
    )
    args = parser.parse_args()

    logger.info(f"--- WeaveLang Pipeline Orchestrator v{args.version} Initializing ---")

    # --- Configuration and Resource Loading ---
    try:
        with open(args.project_config, "rb") as f:
            config = tomllib.load(f)
        content_project_dir_str = config.get("content_project_dir")
        pipeline_config = config.get("pipeline", {})
        models_config = config.get("models", {})
        stages_config = config.get("stages", {})
    except Exception as e:
        logger.critical(f"Failed to load or parse config file '{args.project_config}': {e}")
        sys.exit(1)
    
    if not content_project_dir_str:
        logger.critical("'content_project_dir' not found in config.")
        sys.exit(1)

    logger.info("Initializing shared resources (LLM Client & SpaCy Models)...")
    
    llm_provider = pipeline_config.get("llm_provider", "claude")
    llm_client = helper.initialize_llm_client(llm_provider)
    if llm_client is None:
        sys.exit(1)
        
    spacy_models = {}
    try:
        spacy_models["en"] = spacy.load("en_core_web_lg", disable=["ner"])
        spacy_models["es"] = spacy.load("es_core_news_lg", disable=["ner"])
        logger.info("SpaCy models loaded successfully.")
    except IOError as e:
        logger.critical(f"SpaCy model not found. Have you run 'python -m spacy download ...' for en_core_web_lg and es_core_news_lg? Error: {e}")
        sys.exit(1)

    common_resources = {
        'llm_client': llm_client,
        'spacy_models': spacy_models,
        'content_project_dir': content_project_dir_str,
        'models_config': models_config,
        'pipeline_config': pipeline_config,
        'stages_config': stages_config
    }

    # --- Book Discovery ---
    content_project_root = Path(content_project_dir_str)
    staged_dir = content_project_root / args.input_staged_subdir
    
    book_stems = (
        [args.book_to_process] if args.book_to_process 
        else sorted([f.stem for f in staged_dir.glob("*.txt") if not f.name.endswith(".junk.txt")])
    )

    if not book_stems:
        logger.warning(f"No books found to process in specified Staged directory: {staged_dir}")
        return

    logger.info(f"Orchestrator starting. Found {len(book_stems)} book(s) to process.")

    # --- Main Processing Loop ---
    overall_success = True
    for book_stem in book_stems:
        logger.info(f"--- Starting Pipeline for Book: [{book_stem}] ---")
        pipeline_ok = True

        effective_start_stage = args.start_at_stage

        # Resumability check for non-forced books
        if args.force_book != book_stem:
            first_incomplete_stage = 1
            for StageClass in PIPELINE_STAGES:
                instance_to_check = StageClass(book_stem, args, common_resources)
                if not instance_to_check._is_stage_complete():
                    first_incomplete_stage = instance_to_check.stage_number
                    break
                first_incomplete_stage = instance_to_check.stage_number + 1
            
            if first_incomplete_stage > effective_start_stage:
                effective_start_stage = first_incomplete_stage
        
        logger.info(f"Effective start stage for '{book_stem}' is Stage {effective_start_stage}.")
        
        for StageClass in PIPELINE_STAGES:
            stage_instance = StageClass(book_stem, args, common_resources)

            if stage_instance.stage_number < effective_start_stage:
                logger.info(f"Skipping stage {stage_instance.stage_number} ({stage_instance.stage_name}) due to resumability check.")
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
        logger.info("Orchestrator finished successfully for all books.")
        sys.exit(0)
    else:
        logger.error("Orchestrator finished, but one or more books failed processing.")
        sys.exit(1)
if __name__ == "__main__":
    main()