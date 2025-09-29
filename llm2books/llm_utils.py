import logging
import re
import time
from typing import Dict, List, Optional, Any

from .llm_logger import LLMLogger
from . import llm_overrides

logger = logging.getLogger("pipeline")

TEMPERATURE_STEPS = [0.1, 0.4, 0.6]

def validate_parsed_llm_response(parsed_data: Dict[str, str], parser_type: str):
    # ... (this function is unchanged) ...
    if parser_type == 'multi_line':
        for s_id, content in parsed_data.items():
            bad_lines = []
            for line in content.splitlines():
                if '->' in line:
                    parts = line.split('->', 1)
                    if len(parts) < 2 or not parts[1].strip():
                        bad_lines.append(line.strip())
            
            if bad_lines:
                error_message = (
                    f"Validation failed for S_ID '{s_id}'. Found {len(bad_lines)} "
                    f"mapping lines with empty translations:\n"
                    f"  - " + "\n  - ".join(bad_lines)
                )
                raise ValueError(error_message)


def run_llm_batch_job(
    llm_client: any,
    job_name: str,
    system_prompt: str,
    items_to_process: List[Dict],
    llm_logger: LLMLogger,
    parser_type: str,
    stage_config: Dict[str, Any],
    models_config: Dict[str, Any],
    pipeline_config: Dict[str, Any]
) -> Optional[List[Dict]]:
    # ... (initial part of function is unchanged) ...
    if not items_to_process:
        return []

    max_retries = stage_config.get("max_api_retries", 3)
    retry_delay = stage_config.get("retry_delay", 7)
    
    primary_model_key = stage_config.get("primary_model")
    fallback_model_key = stage_config.get("fallback_model")
    primary_model_name = models_config.get(primary_model_key, {}).get("name")
    fallback_model_name = models_config.get(fallback_model_key, {}).get("name") if fallback_model_key else None
    
    if not primary_model_name:
        logger.error(f"Model config error for stage '{job_name}': Could not find model for key '{primary_model_key}'")
        return None

    user_prompt = "\n".join([f"{item['id']}: {item['text']}" for item in items_to_process])
    prompt_ids = [item['id'] for item in items_to_process]
    
    # --- THIS IS THE NEW CONFIG KEY ---
    thinking_on_first = pipeline_config.get("thinking_on_first_attempt", False)

    for attempt in range(max_retries):
        model_to_use = primary_model_name
        temp_index = min(attempt, len(TEMPERATURE_STEPS) - 1)
        current_temperature = TEMPERATURE_STEPS[temp_index]

        if attempt == max_retries - 1 and fallback_model_name:
            model_to_use = fallback_model_name
            current_temperature = TEMPERATURE_STEPS[0]
            logger.info(f"    -> Final attempt. Switching to fallback model '{model_to_use}'.")

        logger_temp_str = f"{current_temperature}"

        api_payload = {
            "model": model_to_use,
            "system": system_prompt,
            "messages": [{"role": "user", "content": user_prompt}],
            "max_tokens": 4096,
        }

        thinking_budget = pipeline_config.get("thinking_budget_tokens", 0)

        # --- THIS LOGIC IS NOW UPDATED TO USE THE NEW CONFIG KEY ---
        enable_thinking = (attempt > 0 or thinking_on_first) and thinking_budget > 0

        if enable_thinking:
            if attempt == 0:
                logger.info(f"    -> Thinking on first attempt ENABLED. Budget: {thinking_budget} tokens.")
            else:
                logger.info(f"    -> Enabling extended thinking on retry. Budget: {thinking_budget} tokens.")
            
            api_payload["thinking"] = { "type": "enabled", "budget_tokens": thinking_budget }
            current_temperature = 1.0
            logger_temp_str = f"{current_temperature} (required for thinking)"
        
        api_payload["temperature"] = current_temperature
        
        logger.info(f"    -> Running {job_name} LLM batch (Attempt {attempt + 1}/{max_retries}) for {len(items_to_process)} items, using model '{model_to_use}' with temp={logger_temp_str}...")
        
        # ... (rest of the function is unchanged) ...
        raw_response_text = ""
        try:
            message = llm_client.messages.create(**api_payload)
            
            full_raw_response_for_log = ""
            if message.content:
                for block in message.content:
                    if block.type == 'thinking':
                        full_raw_response_for_log += f"--- THINKING ---\n{block.thinking}\n--- END THINKING ---\n"
                    elif block.type == 'text':
                        full_raw_response_for_log += block.text
                        if not raw_response_text:
                            raw_response_text = block.text
            
            llm_logger.log_batch(job_name, 0, system_prompt, user_prompt, full_raw_response_for_log)
            
            if parser_type == 'multi_line':
                parsed_response = _parse_structured_llm_response(raw_response_text, prompt_ids)
            else:
                parsed_response = _parse_singleline_llm_response(raw_response_text)
            
            missing_ids = [pid for pid in prompt_ids if pid not in parsed_response or not parsed_response[pid].strip()]
            
            if missing_ids:
                logger.warning(f"      -> {job_name} batch validation failed. Missing or empty responses for IDs: {missing_ids}.")
            else:
                try:
                    validate_parsed_llm_response(parsed_response, parser_type)
                    logger.info("      -> Batch successfully processed and validated.")
                    for item in items_to_process:
                        item['llm_response'] = parsed_response[item['id']]
                    return items_to_process
                except ValueError as e:
                    logger.warning(f"      -> {job_name} batch content validation failed. Reason: {e}")

            if attempt < max_retries - 1:
                logger.warning("         Retrying entire batch...")
                time.sleep(retry_delay)

        except Exception as e:
            logger.error(f"      -> API Error during {job_name} batch with model '{model_to_use}': {e}", exc_info=False)
            if attempt < max_retries - 1:
                logger.warning("         Retrying entire batch due to API error...")
                time.sleep(retry_delay)
    
    logger.error(f"LLM batch failed for {job_name} after {max_retries} attempts. Halting pipeline.")
    return None

def _parse_singleline_llm_response(raw_text: str) -> Dict[str, str]:
    parsed = {}
    line_regex = re.compile(r"^\s*([^:]+):\s*(.*)$")
    for line in raw_text.splitlines():
        match = line_regex.match(line)
        if match:
            parsed[match.group(1).strip()] = match.group(2).strip()
    return parsed

def _parse_structured_llm_response(raw_text: str, expected_ids: List[str]) -> Dict[str, str]:
    parsed = {}
    id_pattern = "|".join(re.escape(id) for id in expected_ids)
    block_splitter = re.compile(rf"(?=^\s*(?:{id_pattern})\s*:)", re.MULTILINE)
    blocks = [b for b in block_splitter.split(raw_text) if b.strip()]
    for block in blocks:
        lines = block.strip().splitlines()
        if not lines: continue
        first_line_parts = lines[0].split(':', 1)
        if len(first_line_parts) != 2: continue
        current_id = first_line_parts[0].strip()
        if current_id not in expected_ids: continue
        collecting, buffer = False, []
        for line in lines[1:]:
            line_upper_stripped = line.strip().upper()
            if line_upper_stripped.startswith("MAPPINGS:"):
                collecting = True
                if len(line.strip()) > len("MAPPINGS:"):
                    buffer.append(line.split(":", 1)[1].strip())
                continue
            if line_upper_stripped.startswith("VALIDATION:"):
                collecting = False
                break 
            if collecting:
                buffer.append(line)
        if buffer:
            parsed[current_id] = "\n".join(buffer).strip()
    return parsed