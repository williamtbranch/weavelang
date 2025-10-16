import logging
import re
import time
from typing import Dict, List, Optional, Any, Callable
from .llm_logger import LLMLogger
from . import llm_overrides

logger = logging.getLogger("pipeline")

TEMPERATURE_STEPS = [0.1, 0.4, 0.6]

def validate_parsed_llm_response(parsed_data: Dict[str, str], parser_type: str):
    """
    Validates that a parsed LLM response doesn't contain obvious errors,
    like empty mappings, which indicate a malformed response.
    """
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
    llm_clients: Dict[str, any], # <-- MODIFIED: Accepts the dictionary of clients
    job_name: str,
    system_prompt: str,
    items_to_process: List[Dict],
    llm_logger: LLMLogger,
    parser_type: str,
    stage_config: Dict[str, Any],
    models_config: Dict[str, Any],
    pipeline_config: Dict[str, Any],
    post_process_validator: Optional[Callable[[Dict[str, str], List[Dict]], None]] = None
) -> Optional[List[Dict]]:
    
    if not items_to_process:
        return []

    max_retries = stage_config.get("max_api_retries", 3)
    retry_delay = stage_config.get("retry_delay", 7)
    
    primary_model_key = stage_config.get("primary_model")
    fallback_model_key = stage_config.get("fallback_model")
    
    # --- START: NEW MULTI-PROVIDER LOGIC ---
    # Look up the full info for each model from the config
    primary_model_info = models_config.get(primary_model_key, {})
    fallback_model_info = models_config.get(fallback_model_key, {}) if fallback_model_key else {}
    
    primary_model_name = primary_model_info.get("name")
    primary_provider = primary_model_info.get("provider")
    
    fallback_model_name = fallback_model_info.get("name")
    fallback_provider = fallback_model_info.get("provider")

    if not primary_model_name or not primary_provider:
        logger.error(f"Model config error for stage '{job_name}': Could not find name or provider for key '{primary_model_key}'")
        return None
    # --- END: NEW MULTI-PROVIDER LOGIC ---

    user_prompt = "\n".join([f"{item['id']}: {item['text']}" for item in items_to_process])
    prompt_ids = [item['id'] for item in items_to_process]
    
    thinking_on_first = stage_config.get(
        "thinking_on_first_attempt", 
        pipeline_config.get("thinking_on_first_attempt", False)
    )

    for attempt in range(max_retries):
        model_to_use = primary_model_name
        provider_to_use = primary_provider
        temp_index = min(attempt, len(TEMPERATURE_STEPS) - 1)
        current_temperature = TEMPERATURE_STEPS[temp_index]

        if attempt == max_retries - 1 and fallback_model_name:
            model_to_use = fallback_model_name
            provider_to_use = fallback_provider
            current_temperature = TEMPERATURE_STEPS[0]
            logger.info(f"    -> Final attempt. Switching to fallback model '{model_to_use}' ({provider_to_use}).")
        
        # Select the correct client for this attempt
        llm_client = llm_clients.get(provider_to_use)
        if not llm_client:
            logger.error(f"No initialized client found for provider '{provider_to_use}'. Halting.")
            return None

        logger_temp_str = f"{current_temperature}"

        logger.info(f"    -> Running {job_name} LLM batch (Attempt {attempt + 1}/{max_retries}) for {len(items_to_process)} items, using model '{model_to_use}' ({provider_to_use}) with temp={logger_temp_str}...")
        
        raw_response_text = ""
        full_raw_response_for_log = ""

        try:
            # --- START: PROVIDER-AWARE API CALL BLOCK ---
            if provider_to_use == 'claude':
                api_payload = {
                    "model": model_to_use,
                    "system": system_prompt,
                    "messages": [{"role": "user", "content": user_prompt}],
                    "max_tokens": 4096,
                    "temperature": current_temperature,
                }
                
                # Claude-specific "thinking" logic
                thinking_budget = pipeline_config.get("thinking_budget_tokens", 0)
                enable_thinking = (attempt > 0 or thinking_on_first) and thinking_budget > 0
                if enable_thinking:
                    api_payload["thinking"] = { "type": "enabled", "budget_tokens": thinking_budget }
                    api_payload["temperature"] = 1.0 # Temperature must be > 0 for thinking
                    logger.info(f"    -> Enabling extended thinking on retry. Budget: {thinking_budget} tokens. Temp set to 1.0.")

                message = llm_client.messages.create(**api_payload)
                
                # Process Claude's block-based response for logging
                if message.content:
                    for block in message.content:
                        if block.type == 'thinking':
                            full_raw_response_for_log += f"--- THINKING ---\n{block.thinking}\n--- END THINKING ---\n"
                        elif block.type == 'text':
                            full_raw_response_for_log += block.text
                            if not raw_response_text: # Capture the first text block as the main response
                                raw_response_text = block.text

            elif provider_to_use == 'gemini':
                # Gemini has a simpler API structure for this use case
                model = llm_client.GenerativeModel(model_to_use)
                response = model.generate_content(
                    [system_prompt, user_prompt],
                    generation_config={"temperature": current_temperature}
                )
                raw_response_text = response.text
                full_raw_response_for_log = raw_response_text

            else:
                raise ValueError(f"Unsupported provider '{provider_to_use}' in llm_utils.")
            # --- END: PROVIDER-AWARE API CALL BLOCK ---

            llm_logger.log_batch(job_name, 0, system_prompt, user_prompt, full_raw_response_for_log)
            
            # --- This section is now common to both providers ---
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
    # This regex is a positive lookahead, ensuring it splits *before* the next ID.
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
        
        # Start buffer with any content on the first line after the colon
        if first_line_parts[1].strip():
             buffer.append(first_line_parts[1].strip())

        for line in lines[1:]:
            line_upper_stripped = line.strip().upper()
            if line_upper_stripped.startswith("MAPPINGS:"):
                collecting = True
                # If content exists on the same line as MAPPINGS:
                if len(line.strip()) > len("MAPPINGS:"):
                    buffer.append(line.split(":", 1)[1].strip())
                continue # Don't append the "MAPPINGS:" line itself

            if line_upper_stripped.startswith("VALIDATION:"):
                collecting = False
                break 
            
            if collecting:
                buffer.append(line)
        
        if buffer:
            parsed[current_id] = "\n".join(buffer).strip()

    return parsed