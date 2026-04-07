# llm2books/llm_utils.py
import logging
import re
import time
from typing import Dict, List, Optional, Any, Callable
from .llm_logger import LLMLogger
from . import llm_overrides

logger = logging.getLogger("pipeline")

TEMPERATURE_STEPS = [0.1, 0.4, 0.6]


def fix_possessive_splits_in_lines(mapping_lines: List[str]) -> List[str]:
    """
    Detects and corrects a common LLM error where possessive nouns are split
    across two mapping lines, e.g.:
        Hugson -> {{Hugson}}
        s -> NO_SUB
    Corrects to:
        Hugson's -> {{Hugson}}
    """
    fixed = []
    i = 0
    while i < len(mapping_lines):
        line = mapping_lines[i]
        if i + 1 < len(mapping_lines) and '->' in line:
            next_line = mapping_lines[i + 1]
            if '->' in next_line:
                next_parts = next_line.split('->', 1)
                next_lhs = next_parts[0].strip()
                next_rhs = next_parts[1].strip() if len(next_parts) == 2 else ""
                if next_lhs in ("s", "'s") and next_rhs.upper() == "NO_SUB":
                    parts = line.split('->', 1)
                    if len(parts) == 2:
                        lhs = parts[0].strip()
                        rhs = parts[1].strip()
                        fixed.append(f"{lhs}'s -> {rhs}")
                        logger.info(f"      -> Auto-fixed possessive split: '{lhs}' + '{next_lhs}' merged to '{lhs}'s'")
                        i += 2
                        continue
        fixed.append(line)
        i += 1
    return fixed


def fix_possessive_splits(mapping_text: str) -> str:
    """String wrapper for fix_possessive_splits_in_lines."""
    lines = mapping_text.splitlines()
    fixed = fix_possessive_splits_in_lines(lines)
    return "\n".join(fixed)

# --- UPDATED CACHE FUNCTION ---
def create_gemini_cache(model_name: str, system_prompt: str, llm_client: Any = None) -> Optional[Any]:
    """
    Creates a cached content object for a Gemini model.
    """
    try:
        # Use the passed client if it's the genai module, or fallback to import
        genai = llm_client
        if not genai:
            logger.warning("      -> create_gemini_cache called without llm_client, attempting fallback import.")
            import google.generativeai as genai

        # The caching API often requires the 'models/' prefix.
        if not model_name.startswith("models/"):
            full_model_name = f"models/{model_name}"
        else:
            full_model_name = model_name
        
        # Ensure the TTL is under the typical 1-hour token expiry.
        cache = genai.caching.CachedContent.create(
            model=full_model_name,
            system_instruction=system_prompt,
            ttl_seconds=55*60 
        )
        return cache
    except Exception as e:
        # Catch specific argument error for clarity in logs
        if "unexpected keyword argument 'llm_client'" in str(e):
             logger.error(f"      -> Mismatch in create_gemini_cache call. Error: {e}")
        logger.warning(f"      -> Gemini cache creation failed: {e}")
        return None

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


#
def run_llm_batch_job(
    llm_clients: Dict[str, any],
    job_name: str,
    system_prompt: str,
    items_to_process: List[Dict],
    llm_logger: LLMLogger,
    parser_type: str,
    stage_config: Dict[str, Any],
    models_config: Dict[str, Any],
    pipeline_config: Dict[str, Any],
    post_process_validator: Optional[Callable[[Dict[str, str], List[Dict]], None]] = None, # <-- NEW ARGUMENT
    cached_prompt: Optional[Any] = None
) -> Optional[List[Dict]]:
    
    if not items_to_process:
        return []

    max_retries = stage_config.get("max_api_retries", 3)
    retry_delay = stage_config.get("retry_delay", 7)
    
    primary_model_key = stage_config.get("primary_model")
    fallback_model_key = stage_config.get("fallback_model")
    
    primary_model_info = models_config.get(primary_model_key, {})
    fallback_model_info = models_config.get(fallback_model_key, {}) if fallback_model_key else {}
    
    primary_model_name = primary_model_info.get("name")
    primary_provider = primary_model_info.get("provider")
    
    fallback_model_name = fallback_model_info.get("name")
    fallback_provider = fallback_model_info.get("provider")

    if not primary_model_name or not primary_provider:
        logger.error(f"Model config error for stage '{job_name}': Could not find name or provider for key '{primary_model_key}'")
        return None

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
        
        llm_client = llm_clients.get(provider_to_use)
        if not llm_client:
            logger.error(f"No initialized client found for provider '{provider_to_use}'. Halting.")
            return None

        logger_temp_str = f"{current_temperature}"
        logger.info(f"    -> Running {job_name} LLM batch (Attempt {attempt + 1}/{max_retries}) for {len(items_to_process)} items, using model '{model_to_use}' ({provider_to_use}) with temp={logger_temp_str}...")
        
        raw_response_text = ""
        full_raw_response_for_log = ""
        usage_stats = {}

        try:
            log_system_prompt = system_prompt

            if provider_to_use == 'claude':
                # ... (Claude logic is unchanged) ...
                api_payload = { "model": model_to_use, "system": system_prompt, "messages": [{"role": "user", "content": user_prompt}], "max_tokens": 4096, "temperature": current_temperature }
                thinking_budget = pipeline_config.get("thinking_budget_tokens", 0)
                if (attempt > 0 or thinking_on_first) and thinking_budget > 0:
                    api_payload["thinking"], api_payload["temperature"] = { "type": "enabled", "budget_tokens": thinking_budget }, 1.0
                
                message = llm_client.messages.create(**api_payload)
                
                if message.usage:
                    usage_stats['input_tokens'] = message.usage.input_tokens
                    usage_stats['output_tokens'] = message.usage.output_tokens
                usage_stats['cache_status'] = "Automatic (handled by client)"
                
                if message.content:
                    for block in message.content:
                        if block.type == 'thinking': full_raw_response_for_log += f"--- THINKING ---\n{block.thinking}\n--- END THINKING ---\n"
                        elif block.type == 'text':
                            full_raw_response_for_log += block.text
                            if not raw_response_text: raw_response_text = block.text

            elif provider_to_use == 'gemini':
                # ... (Gemini logic is unchanged) ...
                genai = llm_client
                if cached_prompt:
                    model = genai.GenerativeModel.from_cached_content(cached_prompt)
                    content_to_send = [user_prompt]
                else:
                    model = genai.GenerativeModel(model_name=model_to_use, system_instruction=system_prompt)
                    content_to_send = [user_prompt]

                response = model.generate_content(
                    content_to_send, 
                    generation_config={"temperature": current_temperature}
                )
                
                # --- START: NEW SAFETY CHECK ---
                # This handles the "finish_reason: 8" error gracefully.
                if not response.parts:
                    finish_reason = response.candidates[0].finish_reason if response.candidates else "UNKNOWN"
                    raise ValueError(f"Response was empty. Finish Reason: {finish_reason}")
                # --- END: NEW SAFETY CHECK ---

                if hasattr(response, 'usage_metadata'):
                    usage_stats['input_tokens'] = response.usage_metadata.prompt_token_count
                    usage_stats['output_tokens'] = response.usage_metadata.candidates_token_count
                usage_stats['cache_status'] = "Used" if cached_prompt else "Not Used"

                raw_response_text = response.text; full_raw_response_for_log = raw_response_text
            else: 
                raise ValueError(f"Unsupported provider '{provider_to_use}' in llm_utils.")
            
            llm_logger.log_batch(job_name, log_system_prompt, user_prompt, full_raw_response_for_log, usage_stats=usage_stats)
            
            if parser_type == 'multi_line':
                parsed_response = _parse_structured_llm_response(raw_response_text, prompt_ids)
                # Fix common LLM error: possessives split across two lines
                for s_id in parsed_response:
                    parsed_response[s_id] = fix_possessive_splits(parsed_response[s_id])
            else: parsed_response = _parse_singleline_llm_response(raw_response_text)
            
            missing_ids = [pid for pid in prompt_ids if pid not in parsed_response or not parsed_response[pid].strip()]
            if missing_ids: 
                raise ValueError(f"Missing or empty responses for IDs: {missing_ids}")
            
            validate_parsed_llm_response(parsed_response, parser_type)
            
            # --- START: NEW VALIDATOR CALLBACK ---
            # Call the post-processing validator if one was provided.
            # If it raises an error, the outer except block will catch it and trigger a retry/fallback.
            if post_process_validator:
                post_process_validator(parsed_response, items_to_process)
            # --- END: NEW VALIDATOR CALLBACK ---
            
            logger.info("      -> Batch successfully processed and validated.")
            for item in items_to_process: item['llm_response'] = parsed_response[item['id']]
            return items_to_process

        except Exception as e:
            logger.warning(f"      -> {job_name} batch failed on attempt {attempt + 1}. Reason: {e}")
            if attempt < max_retries - 1:
                logger.warning("         Retrying entire batch...")
                time.sleep(retry_delay)
    
    logger.error(f"LLM batch failed for {job_name} after {max_retries} attempts. Halting pipeline.")
    return None


# --- START: RESTORED HELPER FUNCTION ---
def _parse_singleline_llm_response(raw_text: str) -> Dict[str, str]:
    parsed = {}
    line_regex = re.compile(r"^\s*([^:]+):\s*(.*)$")
    for line in raw_text.splitlines():
        match = line_regex.match(line)
        if match:
            parsed[match.group(1).strip()] = match.group(2).strip()
    return parsed
# --- END: RESTORED HELPER FUNCTION ---

# --- START: RESTORED HELPER FUNCTION ---
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
        
        # This simpler logic finds the MAPPINGS: block and takes everything after it
        # until the VALIDATION: block or the end of the block.
        # It's more robust to CoT reasoning appearing before MAPPINGS:.
        in_mappings = False
        buffer = []
        for line in lines:
            stripped_upper = line.strip().upper()
            if stripped_upper.startswith("MAPPINGS:"):
                in_mappings = True
                content_on_same_line = line.strip()[len("MAPPINGS:"):].strip()
                if content_on_same_line:
                    buffer.append(content_on_same_line)
                continue
            
            if in_mappings:
                if stripped_upper.startswith("VALIDATION:"):
                    break # Stop collecting when validation starts
                buffer.append(line)

        if buffer:
            parsed[current_id] = "\n".join(buffer).strip()

    return parsed