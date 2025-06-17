# Filename: orchestrate_pipeline.py
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
    import toml as tomllib
try:
    import spacy
except ImportError:
    print("CRITICAL: SpaCy library not found. Please run `pip install spacy`.", file=sys.stderr)
    sys.exit(1)

# --- Local Module Imports ---
from . import helper
#from .stages.base import Stage # We only need the base for type hinting
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

# --- The Pipeline Stage Registry ---
# This list defines the entire pipeline. To reorder, add, or remove a stage,
# you only need to modify this list.
PIPELINE_STAGES = [
    GenerateAdvSpanish,
    LemmatizeAdvSpanish,
    SegmentAdvSpanish,
    SimplifyAdvSpanish,
    FinalizeSimplerSpanish,
    SegmentEnglish,
    GenerateSimpleSpanish,
    LemmatizeSimpleSpanish,
    GenerateDiglotMap,
    LemmatizeDiglotMap,
]

# --- Logging Setup ---
def get_logger() -> logging.Logger:
    logger = logging.getLogger("pipeline")
    if logger.hasHandlers():
        return logger # Avoid adding duplicate handlers

    log_formatter = logging.Formatter(
        "%(asctime)s - %(levelname)s - %(module)s.%(funcName)s - %(message)s"
    )
    logger.setLevel(logging.INFO)

    console_handler = logging.StreamHandler(sys.stdout)
    console_handler.setLevel(logging.INFO)
    console_handler.setFormatter(log_formatter)
    logger.addHandler(console_handler)

    # ... (add file handlers if desired) ...
    return logger

# --- Main Orchestration Entry Point ---
def main():
    logger = get_logger()
    
    parser = argparse.ArgumentParser(
        description="Orchestrates the WeaveLang multi-stage processing pipeline.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    
    # --- Argument Parsing ---
    parser.add_argument("--project_config", default="config.toml")
    parser.add_argument("--input_staged_subdir", default="Staged")
    parser.add_argument("--output_llm_subdir", default="stage")
    parser.add_argument("--force_book", type=str, default=None)
    parser.add_argument("--book_to_process", type=str, default=None)
    parser.add_argument(
        "--start_at_stage", type=int, default=1, choices=range(1, helper.MAX_STAGES + 1)
    )
    parser.add_argument(
        "--llm_provider", default="claude", choices=["gemini", "claude"]
    )
    parser.add_argument("--llm_model", help="Primary LLM model name.")
    parser.add_argument("--llm_fallback_model", help="Fallback LLM model name.")
    parser.add_argument("--max_sentences_per_batch", type=int, default=5)
    parser.add_argument("--max_api_retries", type=int, default=3)
    parser.add_argument("--max_validation_retries", type=int, default=4)
    parser.add_argument("--retry_delay", type=int, default=7)
    # The version argument was added twice, let's fix that.
    parser.add_argument("--version", default="6.0.0", help="Pipeline version for metadata.")
    args = parser.parse_args()

    logger.info(f"--- WeaveLang Pipeline Orchestrator v{args.version} Initializing ---")


    # --- Configuration and Resource Loading ---
    try:
        with open(args.project_config, "rb") as f:
            config_data = tomllib.load(f)
        content_project_dir_str = config_data.get("content_project_dir")
    except Exception as e:
        logger.critical(f"Failed to load or parse config file '{args.project_config}': {e}")
        sys.exit(1)
    
    if not content_project_dir_str:
        logger.critical("'content_project_dir' not found in config.")
        sys.exit(1)

    logger.info("Initializing shared resources (LLM Client & SpaCy Models)...")
    llm_client = helper.initialize_llm_client(args)
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

    # This defines common_resources
    common_resources = {
        'llm_client': llm_client,
        'spacy_models': spacy_models,
        'content_project_dir': content_project_dir_str
    }

    # --- Book Discovery ---
    content_project_root = Path(content_project_dir_str)
    staged_dir = content_project_root / args.input_staged_subdir
    
    # This defines book_stems
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
        # --- Resumability Logic ---
        # Start with the value from the command line (which defaults to 1).
        effective_start_stage = args.start_at_stage

        # If we are NOT forcing a book, try to find the last completed stage.
        if args.force_book != book_stem:
            latest_complete_stage = 0
            # Loop backwards through the defined stages to find the last completed one.
            for i in range(len(PIPELINE_STAGES), 0, -1):
                StageToCheck = PIPELINE_STAGES[i-1] # Get the class for the stage
                # Temporarily instantiate it to use its completion check method
                instance_to_check = StageToCheck(book_stem, args, common_resources)
                if instance_to_check._is_stage_complete():
                    latest_complete_stage = instance_to_check.stage_number
                    break # Found the last completed stage, no need to check earlier ones
            
            # If we found a completed stage, we should start at the *next* one.
            if latest_complete_stage > 0:
                calculated_start = latest_complete_stage + 1
                # Only use the calculated start if it's further along than a manually passed-in start_at_stage
                if calculated_start > effective_start_stage:
                    effective_start_stage = calculated_start
        
        logger.info(f"Effective start stage for '{book_stem}' is Stage {effective_start_stage}.")
        for StageClass in PIPELINE_STAGES:
            # Add a check for start_at_stage here
            # We need to instantiate to get the stage_number, which is a bit awkward but works.
            temp_instance = StageClass(book_stem, args, common_resources)
            if temp_instance.stage_number < effective_start_stage:
                logger.info(f"Skipping stage {temp_instance.stage_number} ({temp_instance.stage_name}) due to resumability check.")
                logger.info(f"Skipping stage {temp_instance.stage_number} ({temp_instance.stage_name}) due to --start_at_stage setting.")
                continue

            stage_instance = temp_instance # Reuse the instance we just created

            # The resumability and completion check is now handled inside the stage's run() method.
            # The orchestrator's job is just to call them in order.
            
            # Run the stage
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
        sys.exit(0)
    else:
        logger.info("Orchestrator finished, but one or more books failed processing.")
        sys.exit(1)

if __name__ == "__main__":
    main()
