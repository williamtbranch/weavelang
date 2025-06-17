import os
import sys
import argparse
import re
import json
import logging
from datetime import datetime, timezone
from pathlib import Path
from dotenv import load_dotenv
from datetime import datetime, timezone
from typing import Dict, Any, List, Optional, Tuple

try:
    import anthropic
except ImportError:
    anthropic = None

class Helper:
    _instance = None

    # Constants
    MAX_STAGES = 7
    SENTENCE_LINE_REGEX = re.compile(r"^{S(\d+):\s*(.*)}$")
    CHAPTER_MARKER_REGEX = re.compile(r"^%%CHAPTER_MARKER%%\s*(.*)$")
    PROMPT_DIR = Path(__file__).parent / "llm_prompt_templates"
    DEFAULT_CLAUDE_MAX_TOKENS_OUTPUT = 4096

    def __new__(cls):
        if cls._instance is None:
            cls._instance = super().__new__(cls)

        return cls._instance

    def __init__(self):
        log_formatter = logging.Formatter('%(asctime)s - %(levelname)s - %(filename)s:%(lineno)d - %(message)s')
        logger = logging.getLogger(__name__)
        logger.setLevel(logging.INFO)

        if logger.hasHandlers():
            logger.handlers.clear()

        console_handler = logging.StreamHandler(sys.stdout)
        console_handler.setLevel(logging.INFO)
        console_handler.setFormatter(log_formatter)
        logger.addHandler(console_handler)

        file_handler = logging.FileHandler('pipeline_orchestrator.log', mode='w', encoding='utf-8')
        file_handler.setLevel(logging.DEBUG)
        file_handler.setFormatter(log_formatter)
        logger.addHandler(file_handler)

        error_file_handler = logging.FileHandler('pipeline_orchestrator.err', mode='w', encoding='utf-8')
        error_file_handler.setLevel(logging.ERROR)
        error_file_handler.setFormatter(log_formatter)
        logger.addHandler(error_file_handler)

        self.logger = logger

    def get_iso_timestamp():
        return datetime.now(timezone.utc).isoformat().replace('+00:00', 'Z') 

    def initialize_llm_client(args: argparse.Namespace) -> any:
        load_dotenv(dotenv_path=Path('.') / '.env')
        if args.llm_provider == 'claude':
            if not anthropic:
                Helper.logger.critical("Anthropic provider selected, but SDK not installed. `pip install anthropic`"); return None
            api_key = os.getenv("ANTHROPIC_API_KEY")
            if not api_key:
                Helper.logger.critical("ANTHROPIC_API_KEY not found in environment or .env file."); return None
            return anthropic.Anthropic(api_key=api_key)
        return None

    def _load_prompt_template(filename: str) -> Optional[str]:
        file_path = Helper.PROMPT_DIR / filename
        if not file_path.exists():
            Helper.logger.error(f"Prompt template file not found: {file_path}"); return None
        try:
            with open(file_path, 'r', encoding='utf-8') as f: return f.read()
        except Exception as e:
            Helper.logger.error(f"Could not read prompt template file {file_path}: {e}"); return None

    def _make_llm_api_call(llm_client: Any, provider: str, system_prompt: str, user_prompt: str, model_name: str, attempt_num: int, max_attempts: int, max_tokens: int) -> Tuple[Optional[str], Optional[str]]:
        if not llm_client or not user_prompt.strip(): return None, "LLM call skipped: Client not init or user_message is empty."
        try:
            if provider == "claude":
                Helper.logger.info(f"        Making API call to Claude model: {model_name} (Attempt {attempt_num}/{max_attempts})")
                response = llm_client.messages.create(model=model_name, max_tokens=max_tokens, system=system_prompt, messages=[{"role": "user", "content": user_prompt}])
                return response.content[0].text if response.content else None, None
            else: return None, f"Provider '{provider}' not implemented."
        except Exception as e: return None, f"LLM API Error ({provider}, Attempt {attempt_num}/{max_attempts}): {e}"

    def _parse_llm_response_blocks(raw_text: str, expected_ids: List[str]) -> Tuple[Dict[str, str], List[str]]:
        parsed_data, errors = {}, []
        id_pattern = re.compile(r"^(id\s+[\d_A-Za-z]+)\s*:\s*(.*)$", re.IGNORECASE)
        for line in raw_text.splitlines():
            if match := id_pattern.match(line.strip()):
                response_id = " ".join(match.group(1).lower().split())
                if response_id in expected_ids:
                    if response_id in parsed_data: errors.append(f"Duplicate ID found: {response_id}")
                    parsed_data[response_id] = match.group(2).strip().strip('"')
                else: errors.append(f"Received unexpected ID from LLM: {response_id}")
        missing_ids = [eid for eid in expected_ids if eid not in parsed_data]
        if missing_ids: errors.append(f"Missing output for IDs: {', '.join(missing_ids)}")
        return parsed_data, errors

    def _write_error_debug_file(book_stem: str, stage_number: str, error_dir: Path, batch_info: str, last_prompt: str, last_response: str, error_reason: str):
        error_dir.mkdir(parents=True, exist_ok=True)
        file_path = error_dir / f"{book_stem}.stage{stage_number}.err.txt"
        with open(file_path, 'w', encoding='utf-8') as f:
            f.write(f"// WeaveLang Pipeline Error Dump\n// Timestamp: {Helper.get_iso_timestamp()}\n// Book: {book_stem}\n// Stage: {stage_number}\n\n// --- BATCH INFO ---\n{batch_info}\n\n// --- ERROR SUMMARY ---\n{error_reason}\n\n// --- LAST PROMPT SENT TO LLM ---\n{last_prompt}\n\n// --- LAST RAW RESPONSE FROM LLM ---\n{last_response}\n")
        Helper.logger.critical(f"Wrote fatal error details to: {file_path}")

    def get_input_path_for_stage(book_stem: str, stage_num: int, staged_dir: Path, llm_output_base_dir: Path) -> Path:
        if stage_num == 1: return staged_dir / f"{book_stem}.txt"
        else: return llm_output_base_dir / f"stage{stage_num - 1}" / f"{book_stem}.stage{stage_num - 1}.json"

    def is_stage_complete(book_stem: str, stage_num: int, llm_output_base_dir: Path) -> bool:
        json_path = llm_output_base_dir / f"stage{stage_num}" / f"{book_stem}.stage{stage_num}.json"
        if not json_path.exists(): return False
        try:
            with open(json_path, 'r', encoding='utf-8') as f: data = json.load(f)
            return data.get("processing_status") == "COMPLETED"
        except (json.JSONDecodeError, IOError): return False