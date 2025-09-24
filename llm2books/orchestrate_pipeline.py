import argparse
import logging
import sys
import pprint
from .llm_logger import LLMLogger
from pathlib import Path
import re
from typing import Optional, Dict, Any
from .stanza_segmenter import EnglishStanzaProcessor, SpanishStanzaProcessor
from .stages import (
    AssembleTiers, 
    ProcessTargetTiers,
    GeneratePhraseMap, 
    ApplyPhraseMappings, 
    GenerateInverseDiglotMap,
    ApplyInversePhraseMappings,
    FinalizeMappings,
    FinalizeBaseTier,
    FinalizeBook,
)

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
from .stages.base import Stage
from .stanza_segmenter import EnglishStanzaProcessor
from .pool_manager import PoolManager

# The pipeline stages list is now updated with the new Stage 2
PIPELINE_STAGES = [
    AssembleTiers,              # Stage 1
    ProcessTargetTiers,         # Stage 2 (NEW)
    GeneratePhraseMap,          # Stage 3
    ApplyPhraseMappings,        # Stage 4
    GenerateInverseDiglotMap,   # Stage 5
    ApplyInversePhraseMappings, # Stage 6
    FinalizeMappings,           # Stage 7
    FinalizeBaseTier,           # Stage 8
    FinalizeBook,               # Stage 9
]

def get_logger() -> logging.Logger:
    logger = logging.getLogger("pipeline")
    if logger.hasHandlers():
        logger.handlers.clear()

    log_formatter = logging.Formatter(
        "%(asctime)s - %(levelname)s - %(name)s - %(message)s"
    )
    logger.setLevel(logging.DEBUG)

    console_handler = logging.StreamHandler(sys.stdout)
    console_handler.setLevel(logging.DEBUG)
    console_handler.setFormatter(log_formatter)
    logger.addHandler(console_handler)

    return logger

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
    
    parser.add_argument("--project_config", default="config.toml", help="Path to the main project TOML configuration file.")
    parser.add_argument("--book-to-process", required=True, type=str, help="The book stem to process (e.g., 'Grimm').")
    parser.add_argument("--base-lang", required=True, type=str, help="The base language for this run (e.g., 'en').")
    parser.add_argument("--target-lang", required=True, type=str, help="The target language for this run (e.g., 'es').")
    parser.add_argument(
        "--stop-after-stage",
        type=int,
        default=0, # Default to 0, meaning run all stages
        help="If set to a positive integer, the pipeline will stop after completing that stage."
    )
    
    args = parser.parse_args()
    
    logger.info(f"--- WeaveLang Pipeline Orchestrator Initializing (V10 - Multi-Tier) ---")
    logger.info(f"Run configured for Book: '{args.book_to_process}' ({args.base_lang} -> {args.target_lang})")

    # --- Load Configs and Initialize Resources ---
    try:
        with open(args.project_config, "rb") as f: config = tomllib.load(f)
        content_project_dir_str = config.get("content_project_dir")
        if not content_project_dir_str: raise ValueError("'content_project_dir' not found in config.")
        content_project_root = Path(content_project_dir_str)
        
        tool_root_dir = Path(__file__).resolve().parent.parent
        with open(tool_root_dir / "assets" / "languages.toml", "rb") as f: lang_manifest = tomllib.load(f)
        
        # Build language config here to pass to PoolManager
        language_config = build_language_config(lang_manifest, args.base_lang, args.target_lang)
    except Exception as e:
        logger.critical(f"Failed to load configuration files: {e}"); sys.exit(1)

    llm_logger = LLMLogger(content_project_root / "pipeline_runs" / f"{args.base_lang}-{args.target_lang}" / args.book_to_process / "llm_logs")
    logger.info("Initializing shared resources (this may take a moment)...")
    needed_langs = {args.base_lang, args.target_lang}
    spacy_models, stanza_processors = {}, {}
    for lang_code in needed_langs:
        lang_info = lang_manifest.get(lang_code)
        if not lang_info: logger.critical(f"Language '{lang_code}' not defined in languages.toml."); sys.exit(1)
        try:
            spacy_model_name = lang_info.get("spacy_model")
            if spacy_model_name: spacy_models[lang_code] = spacy.load(spacy_model_name, disable=["ner"])
            #
            if lang_code == 'en':
                stanza_processors[lang_code] = EnglishStanzaProcessor(config, llm_logger)
            elif lang_code == 'es':
                stanza_processors[lang_code] = SpanishStanzaProcessor(config, llm_logger)
        except Exception as e:
            logger.critical(f"Failed to load language processors for '{lang_code}': {e}"); sys.exit(1)
    
    llm_client = helper.initialize_llm_client("claude")
    if llm_client is None: sys.exit(1)

    shared_resources = { 
        'llm_client': llm_client,
        'spacy_models': spacy_models,
        'stanza_processors': stanza_processors,
        'models_config': config.get("models", {}),
        'pipeline_config': config.get("pipeline", {}),
        'stages_config': config.get("stages", {}),
        'language_config': language_config,
        'content_project_dir': content_project_dir_str
    }
    pool_manager = PoolManager(content_project_root, shared_resources)

    # --- Main Processing Logic ---
    logger.info(f"--- Starting Resource Generation for Book: [{args.book_to_process}] ---")
    
    book_resources = pool_manager.get_book_resources(args.book_to_process, args.base_lang, args.target_lang)
    
    if book_resources:
        logger.info("--- All required pool files are available. Starting pair-specific pipeline. ---")
        
        shared_resources['book_resources'] = book_resources

        pipeline_ok = True
        overall_success = True
        for StageClass in PIPELINE_STAGES:
            stage_instance = StageClass(args.book_to_process, args, shared_resources)
            
            pipeline_ok = stage_instance.run()

            if not pipeline_ok:
                logger.error(f"Halting pipeline for '{args.book_to_process}' due to failure in stage: {stage_instance.stage_name}.")
                overall_success = False
                break
            
            if args.stop_after_stage > 0 and stage_instance.stage_number == args.stop_after_stage:
                logger.info(f"--- Pipeline stopped as requested after completing Stage {args.stop_after_stage}. ---")
                overall_success = True # This was a successful, planned stop
                break
        
        if overall_success:
             logger.info("Orchestrator finished successfully.")
             sys.exit(0)
        else:
             logger.error(f"--- Pipeline run FAILED for Book: [{args.book_to_process}] ---")
             sys.exit(1)

    else:
        logger.error(f"--- FAILED to generate one or more required pool files for Book: [{args.book_to_process}] ---")
        sys.exit(1)

if __name__ == "__main__":
    main()