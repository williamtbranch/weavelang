# llm2books/orchestrate_pipeline.py
import argparse
import logging
import sys
from pathlib import Path
from typing import Optional, Dict, Any, List

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
from .llm_logger import LLMLogger
from .pool_manager import PoolManager
from .stages.base import Stage

# --- Stage Definitions (Updated for V11 Architecture) ---
from .stages import (
    AssembleTiers,
    GenerateBasicBase,
    TranslateBasicTarget,
    GeneratePhraseMap,
    GenerateInverseDiglotMap,
    ApplyPhraseMappings,
    ApplyInversePhraseMappings,
    FinalizeMappings,
    FinalizeBook,
)

# --- Configuration for the new Human-in-the-Loop workflow ---
HUMAN_REVIEW_DIR_NAME = "human_review"
HUMAN_REVIEW_MARKER = "%%HUMAN_REVIEW_APPROVED%%"

# The pipeline will be broken into distinct phases
PHASE_1_STAGES = [AssembleTiers, GenerateBasicBase]
PHASE_2_STAGES = [TranslateBasicTarget, GeneratePhraseMap, GenerateInverseDiglotMap]
PHASE_3_STAGES = [ApplyPhraseMappings, ApplyInversePhraseMappings, FinalizeMappings, FinalizeBook]


def get_logger() -> logging.Logger:
    logger = logging.getLogger("pipeline")
    if logger.hasHandlers():
        logger.handlers.clear()
    log_formatter = logging.Formatter("%(asctime)s - %(levelname)s - %(name)s - %(message)s")
    logger.setLevel(logging.INFO)
    console_handler = logging.StreamHandler(sys.stdout)
    console_handler.setLevel(logging.INFO)
    console_handler.setFormatter(log_formatter)
    logger.addHandler(console_handler)
    return logger

# Initialize logger at the module level
logger = get_logger()

def check_approval_status(file_path: Path) -> bool:
    if not file_path.is_file():
        return False
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            first_line = f.readline().strip()
        return first_line == HUMAN_REVIEW_MARKER
    except Exception:
        return False

def determine_pipeline_state(
    final_book_path: Path,
    en_review_path: Path,
    dig_review_path: Path,
    invdig_review_path: Path
) -> str:
    if final_book_path.exists():
        return "COMPLETE"
    
    en_approved = check_approval_status(en_review_path)
    dig_approved = check_approval_status(dig_review_path)
    invdig_approved = check_approval_status(invdig_review_path)

    if en_approved and dig_approved and invdig_approved:
        return "MAPPINGS_APPROVED"
    
    if en_approved and (dig_review_path.exists() or invdig_review_path.exists()):
        return "AWAITING_MAPPING_REVIEW"
        
    if en_approved:
        return "ENGLISH_APPROVED"

    if en_review_path.exists():
        return "AWAITING_ENGLISH_REVIEW"
        
    return "START"

def run_pipeline_phase(
    stages_to_run: List[type],
    book_stem: str,
    cli_args: argparse.Namespace,
    shared_resources: Dict[str, Any]
) -> bool:
    for StageClass in stages_to_run:
        stage_instance = StageClass(book_stem, cli_args, shared_resources)
        
        if not stage_instance.run():
            logger.error(f"Halting pipeline due to failure in stage: {stage_instance.stage_name}.")
            return False
            
        if cli_args.stop_after_stage > 0 and stage_instance.stage_number == cli_args.stop_after_stage:
            logger.info(f"--- Pipeline stopped as requested after completing Stage {cli_args.stop_after_stage}. ---")
            break
    
    return True

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

# --- Main Orchestration Entry Point ---
def main():
    parser = argparse.ArgumentParser(
        description="Orchestrates the WeaveLang data generation pipeline (V11 - Human-in-the-Loop).",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter
    )
    parser.add_argument("--project_config", default="config.toml", help="Path to the main project TOML configuration file.")
    parser.add_argument("--book-to-process", required=True, type=str, help="The book stem to process (e.g., 'Grimm').")
    parser.add_argument("--base-lang", required=True, type=str, help="The base language for this run (e.g., 'en').")
    parser.add_argument("--target-lang", required=True, type=str, help="The target language for this run (e.g., 'es').")
    parser.add_argument("--stop-after-stage", type=int, default=0, help="Stop after a specific stage number (useful for debugging phases).")
    
    args = parser.parse_args()
    
    logger.info(f"--- WeaveLang Pipeline Orchestrator Initializing (V11) ---")
    logger.info(f"Run configured for Book: '{args.book_to_process}' ({args.base_lang} -> {args.target_lang})")

    try:
        with open(args.project_config, "rb") as f: config = tomllib.load(f)
        content_project_dir_str = config.get("content_project_dir")
        if not content_project_dir_str: raise ValueError("'content_project_dir' not found in config.")
        content_project_root = Path(content_project_dir_str)
    except Exception as e:
        logger.critical(f"Failed to load configuration files: {e}"); sys.exit(1)

    lang_pair_dir = f"{args.base_lang}-{args.target_lang}"
    pipeline_run_dir = content_project_root / "pipeline_runs" / lang_pair_dir / args.book_to_process
    
    human_review_dir = pipeline_run_dir / HUMAN_REVIEW_DIR_NAME
    final_book_path = content_project_root / "library" / f"{args.book_to_process}.json"
    en_review_path = human_review_dir / f"{args.book_to_process}.basic_en.txt"
    dig_review_path = human_review_dir / f"{args.book_to_process}.dig.txt"
    invdig_review_path = human_review_dir / f"{args.book_to_process}.invdig.txt"

    pipeline_state = determine_pipeline_state(final_book_path, en_review_path, dig_review_path, invdig_review_path)
    logger.info(f"Detected pipeline state: {pipeline_state}")
    
    stages_to_run = []
    if pipeline_state == "START":
        logger.info("--- Starting PHASE 1: Generating 'basic_english' for review. ---")
        stages_to_run = PHASE_1_STAGES
    elif pipeline_state == "AWAITING_ENGLISH_REVIEW":
        logger.info("--- PAUSED: Waiting for human review of 'basic_english'. ---")
        logger.info(f"Please edit the file below and uncomment the approval marker on the first line:")
        logger.info(f"  -> {en_review_path.resolve()}")
        sys.exit(0)
    elif pipeline_state == "ENGLISH_APPROVED":
        logger.info("--- Starting PHASE 2: Translating and generating mappings for review. ---")
        stages_to_run = PHASE_2_STAGES
    elif pipeline_state == "AWAITING_MAPPING_REVIEW":
        logger.info("--- PAUSED: Waiting for human review of mapping files. ---")
        logger.info("Please review/edit the files and uncomment their approval markers:")
        if not check_approval_status(dig_review_path): logger.info(f"  -> PENDING: {dig_review_path.resolve()}")
        if not check_approval_status(invdig_review_path): logger.info(f"  -> PENDING: {invdig_review_path.resolve()}")
        sys.exit(0)
    elif pipeline_state == "MAPPINGS_APPROVED":
        logger.info("--- Starting PHASE 3: Consuming reviewed mappings and finalizing book. ---")
        stages_to_run = PHASE_3_STAGES
    elif pipeline_state == "COMPLETE":
        logger.info("--- Pipeline for this book is already complete. Nothing to do. ---")
        sys.exit(0)

    if not stages_to_run:
        logger.error(f"Could not determine stages to run for state '{pipeline_state}'. Halting.")
        sys.exit(1)

    logger.info("Initializing shared resources...")
    llm_logger = LLMLogger(pipeline_run_dir / "llm_logs")
    providers_in_use = set(model_info.get("provider") for model_info in config.get("models", {}).values())
    llm_clients = {p: helper.initialize_llm_client(p) for p in providers_in_use if p}

    try:
        tool_root_dir = Path(__file__).resolve().parent.parent
        with open(tool_root_dir / "assets" / "languages.toml", "rb") as f:
            lang_manifest = tomllib.load(f)
        
        language_config = build_language_config(lang_manifest, args.base_lang, args.target_lang)

        # --- THIS IS THE SIMPLIFIED AND CORRECTED LOGIC ---
        spacy_models = {}
        base_model_name = language_config.get("base_spacy_model")
        target_model_name = language_config.get("target_spacy_model")

        if base_model_name:
            logger.info(f"  -> Loading SpaCy model for base language '{args.base_lang}': '{base_model_name}'")
            spacy_models[args.base_lang] = spacy.load(base_model_name, disable=["ner"])
        
        #
        if target_model_name:
            logger.info(f"  -> Loading SpaCy model for target language '{args.target_lang}': '{target_model_name}'")
            spacy_models[args.target_lang] = spacy.load(target_model_name, disable=["ner"])
        
        # --- START OF FIX ---
        # We also need the Stanza processors for the PoolManager
        from .stanza_segmenter import EnglishStanzaProcessor, SpanishStanzaProcessor
        stanza_processors = {}
        logger.info(f"  -> Initializing Stanza processor for base language '{args.base_lang}'")
        if args.base_lang == 'en':
            stanza_processors['en'] = EnglishStanzaProcessor(config, llm_logger)
        elif args.base_lang == 'es':
            stanza_processors['es'] = SpanishStanzaProcessor(config, llm_logger)
        
        logger.info(f"  -> Initializing Stanza processor for target language '{args.target_lang}'")
        if args.target_lang == 'en':
            stanza_processors['en'] = EnglishStanzaProcessor(config, llm_logger)
        elif args.target_lang == 'es':
            stanza_processors['es'] = SpanishStanzaProcessor(config, llm_logger)
        # --- END OF FIX ---

    except Exception as e:
        logger.critical(f"Failed to load language resources (manifest, spacy, stanza): {e}"); sys.exit(1)


    shared_resources = {
        'llm_clients': llm_clients, 'spacy_models': spacy_models,
        'stanza_processors': stanza_processors, # <-- ADD THIS KEY
        'models_config': config.get("models", {}), 'pipeline_config': config.get("pipeline", {}),
        'stages_config': config.get("stages", {}), 'content_project_dir': content_project_dir_str,
        'llm_logger': llm_logger, 'language_config': language_config,
    }
    
    pool_manager = PoolManager(content_project_root, shared_resources)
    book_resources = pool_manager.get_book_resources(args.book_to_process, args.base_lang, args.target_lang)
    
    logger.info(f"DEBUG: PoolManager returned book_resources: {book_resources}")
    
    if not book_resources:
        logger.error(f"PoolManager failed to get or create required book resources. Halting.")
        sys.exit(1)
    
    shared_resources['book_resources'] = book_resources
    
    success = run_pipeline_phase(stages_to_run, args.book_to_process, args, shared_resources)

    if success:
        logger.info("--- Pipeline phase completed successfully. ---")
        if pipeline_state == "START":
             logger.info("ACTION REQUIRED: Please review and approve the generated 'basic_english' file to continue.")
        elif pipeline_state == "ENGLISH_APPROVED":
             logger.info("ACTION REQUIRED: Please review and approve the generated mapping files to continue.")
        sys.exit(0)
    else:
        logger.error(f"--- Pipeline run FAILED for Book: [{args.book_to_process}] ---")
        sys.exit(1)

if __name__ == "__main__":
    main()