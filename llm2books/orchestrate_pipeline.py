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
# --- Local Module Imports ---
from . import helper
# --- MODIFIED: Import the new, generically named classes ---
from .stages.generate_advanced_target import GenerateAdvancedTarget
from .stages.lemmatize_advanced_target import LemmatizeAdvancedTarget
from .stages.segment_advanced_target import SegmentAdvancedTarget
from .stages.simplify_advanced_target import SimplifyAdvancedTarget
from .stages.finalize_simpler_target import FinalizeSimplerTarget
from .stages.segment_base import SegmentBase
from .stages.generate_simple_target import GenerateSimpleTarget
from .stages.lemmatize_simple_target import LemmatizeSimpleTarget
from .stages.generate_diglot_map import GenerateDiglotMap
from .stages.lemmatize_diglot_map import LemmatizeDiglotMap
from .stages.generate_inverse_diglot_map import GenerateInverseDiglotMap
from .stages.lemmatize_inverse_diglot_map import LemmatizeInverseDiglotMap


# --- The Pipeline Stage Registry ---
# --- MODIFIED: The registry now uses the generic names ---
PIPELINE_STAGES = [
    GenerateAdvancedTarget,      # Stage 1
    LemmatizeAdvancedTarget,     # Stage 2
    SegmentAdvancedTarget,       # Stage 3a
    SimplifyAdvancedTarget,      # Stage 3b
    FinalizeSimplerTarget,       # Stage 4
    SegmentBase,                 # Stage 5a
    GenerateSimpleTarget,        # Stage 5b
    LemmatizeSimpleTarget,       # Stage 6
    GenerateDiglotMap,           # Stage 7
    LemmatizeDiglotMap,          # Stage 8
    GenerateInverseDiglotMap,    # Stage 9
    LemmatizeInverseDiglotMap,   # Stage 10
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
    parser.add_argument("--version", default="8.0.0-language-agnostic", help="Pipeline version for metadata.")
    parser.add_argument("--input_staged_subdir", default="Staged", help="Subdirectory for initial staged text files.")
    parser.add_argument("--output_llm_subdir", default="pipeline", help="Base subdirectory for intermediate pipeline stage outputs.")
    
    # --- NEW: Language pair arguments ---
    parser.add_argument("--base-lang", required=True, help="Base language code (e.g., 'en')")
    parser.add_argument("--target-lang", required=True, help="Target language code (e.g., 'es')")

    # --- Execution control arguments ---
    parser.add_argument("--force_book", type=str, default=None, help="Force reprocessing of a specific book, ignoring existing progress.")
    parser.add_argument("--book_to_process", type=str, default=None, help="Process only a single specified book.")
    parser.add_argument(
        "--start_at_stage", type=int, default=1, choices=range(1, len(PIPELINE_STAGES) + 2), help="Start processing from a specific stage number."
    )
    parser.add_argument(
        "--run_only_stage", type=int, default=None, choices=range(1, len(PIPELINE_STAGES) + 1), help="If specified, run ONLY this single stage and then exit."
    )
    args = parser.parse_args()

    logger.info(f"--- WeaveLang Pipeline Orchestrator v{args.version} Initializing ---")
    logger.info(f"Processing language pair: {args.base_lang} -> {args.target_lang}")

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

    # --- NEW: Load language-specific assets based on new structure ---
    tool_root_dir = Path(__file__).resolve().parent.parent
    content_project_root = Path(content_project_dir_str)
    
    lang_manifest_path = tool_root_dir / "assets" / "languages.toml"
    try:
        with open(lang_manifest_path, "rb") as f:
            lang_manifest = tomllib.load(f)
    except Exception as e:
        logger.critical(f"Failed to load language manifest at '{lang_manifest_path}': {e}")
        sys.exit(1)

    # --- NEW: Build the language_config object for this run ---
    lang_pair_key = f"{args.base_lang}-{args.target_lang}"
    base_lang_conf = lang_manifest.get(args.base_lang, {})
    target_lang_conf = lang_manifest.get(args.target_lang, {})
    pair_conf = lang_manifest.get("pair", {}).get(lang_pair_key, {})

    if not all([base_lang_conf, target_lang_conf, pair_conf]):
        logger.critical(f"Language configuration for '{lang_pair_key}' not found or incomplete in languages.toml.")
        sys.exit(1)

    language_config = {
        "base_code": args.base_lang,
        "target_code": args.target_lang,
        "base_spacy_model": base_lang_conf.get("spacy_model"),
        "target_spacy_model": target_lang_conf.get("spacy_model"),
        "prompt_dir": tool_root_dir / "assets" / pair_conf.get("prompt_dir")
    }

    # --- MODIFIED: Load SpaCy models dynamically ---
    logger.info("Initializing shared resources (LLM Client & SpaCy Models)...")
    spacy_models = {}
    try:
        spacy_models[args.base_lang] = spacy.load(language_config["base_spacy_model"], disable=["ner"])
        spacy_models[args.target_lang] = spacy.load(language_config["target_spacy_model"], disable=["ner"])
        logger.info("SpaCy models loaded successfully.")
    except IOError as e:
        logger.critical(f"SpaCy model not found. Have you run 'python -m spacy download ...'? Error: {e}")
        sys.exit(1)

    # --- MODIFIED: Resolve frequency list path (custom or default) ---
    custom_freq_path_str = config.get("custom_frequency_list_path", "").strip()
    if custom_freq_path_str:
        freq_list_path = Path(custom_freq_path_str)
        logger.info(f"Using custom frequency list from config: {freq_list_path}")
    else:
        freq_list_path = tool_root_dir / "assets" / "frequency_lists" / f"{args.target_lang}_master_frequency_list.txt"
        logger.info(f"Using default frequency list for '{args.target_lang}': {freq_list_path}")

    if not freq_list_path.is_file():
        logger.critical(f"Frequency list not found at resolved path: {freq_list_path}")
        sys.exit(1)

    # --- MODIFIED: Dynamically determine the LLM provider and assemble common resources ---
    # Find the first LLM provider listed in any stage to initialize the client.
    # This assumes all stages in a single run use the same provider (e.g., all Claude, or all Gemini), which is the current design.
    llm_provider = None
    for stage_class in PIPELINE_STAGES:
        # Use the class name (e.g., "GenerateAdvSpanish") to look up its config
        stage_conf = stages_config.get(stage_class.__name__, {})
        primary_model_key = stage_conf.get("primary_model")
        if primary_model_key:
            model_conf = models_config.get(primary_model_key, {})
            if "provider" in model_conf:
                llm_provider = model_conf["provider"]
                logger.info(f"Identified LLM provider '{llm_provider}' from config for stage '{stage_class.__name__}'.")
                break # Found the first one, so we can stop looking.

    if not llm_provider:
        logger.warning("Could not determine LLM provider from stage configurations. Defaulting to 'claude'.")
        llm_provider = "claude"

    # Initialize the client using the dynamically found provider
    llm_client = helper.initialize_llm_client(llm_provider)
    if llm_client is None:
        sys.exit(1)

    # Assemble the final common_resources dictionary to pass to all stages
    common_resources = {
        'llm_client': llm_client,
        'spacy_models': spacy_models,
        'content_project_dir': content_project_dir_str,
        'models_config': models_config,
        'pipeline_config': pipeline_config,
        'stages_config': stages_config,
        'language_config': language_config, # NEW
        'frequency_list_path': freq_list_path # NEW
    }
    # --- Book Discovery ---
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
    # NOTE: The V1 to V2 migration logic will be added here in a later step.
    # For now, we assume a clean slate for the new schema.
    overall_success = True
    for book_stem in book_stems:
        logger.info(f"--- Starting Pipeline for Book: [{book_stem}] ---")
        pipeline_ok = True

        effective_start_stage = args.start_at_stage

        if args.run_only_stage is not None:
            effective_start_stage = args.run_only_stage
        elif args.force_book != book_stem:
            # Resumability check will need to be updated for V2 schema
            pass # Placeholder for now
        
        logger.info(f"Effective start stage for '{book_stem}' is Stage {effective_start_stage}.")
        if args.run_only_stage is not None:
            logger.info(f"Execution is limited to Stage {args.run_only_stage} ONLY.")
        
        for StageClass in PIPELINE_STAGES:
            stage_instance = StageClass(book_stem, args, common_resources)

            if args.run_only_stage is not None and stage_instance.stage_number != args.run_only_stage:
                continue
            elif stage_instance.stage_number < effective_start_stage:
                logger.info(f"Skipping stage {stage_instance.stage_number} ({stage_instance.stage_name}) due to start_at_stage setting.")
                continue

            # TODO: This will fail until we refactor the stages. This is the next step.
            pipeline_ok = stage_instance.run()

            if not pipeline_ok or args.run_only_stage is not None:
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