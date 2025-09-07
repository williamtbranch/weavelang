import logging
import re
import time
from typing import Dict, List, Optional, Any

from .llm_logger import LLMLogger

logger = logging.getLogger("pipeline")

def run_llm_batch_job(
    llm_client: any,
    job_name: str,
    system_prompt: str,
    items_to_process: List[Dict],
    llm_logger: LLMLogger,
    parser_type: str,
    stage_config: Dict[str, Any],
    models_config: Dict[str, Any]
) -> Optional[List[Dict]]:
    """
    Handles an LLM job for a batch of items with robust parsing, validation, and retries.
    This version uses a robust parser and fails fast if validation does not pass after all retries.
    """
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
    
    for attempt in range(max_retries):
        model_to_use = primary_model_name
        if attempt == max_retries - 1 and fallback_model_name:
            model_to_use = fallback_model_name
            logger.info(f"    -> Final attempt. Switching to fallback model '{model_to_use}'.")

        logger.info(f"    -> Running {job_name} LLM batch (Attempt {attempt + 1}/{max_retries}) using model '{model_to_use}'...")
        
        raw_response = ""
        try:
            message = llm_client.messages.create(
                model=model_to_use, 
                system=system_prompt, 
                messages=[{"role": "user", "content": user_prompt}], 
                max_tokens=4096, 
                temperature=0.1
            )
            raw_response = message.content[0].text if message.content else ""
            llm_logger.log_batch(job_name, 0, system_prompt, user_prompt, raw_response)
            
            # Use the correct parser based on the stage's needs
            if parser_type == 'multi_line':
                # Pass the raw response and the list of IDs we expect to find
                parsed_response = _parse_structured_llm_response(raw_response, prompt_ids)
            else:
                parsed_response = _parse_singleline_llm_response(raw_response)
            
            # --- FAIL-FAST VALIDATION ---
            missing_ids = [pid for pid in prompt_ids if pid not in parsed_response or not parsed_response[pid].strip()]

            if not missing_ids:
                logger.info("      -> Batch successfully processed and validated.")
                for item in items_to_process:
                    item['llm_response'] = parsed_response[item['id']]
                return items_to_process # SUCCESS!
            else:
                logger.warning(f"      -> {job_name} batch validation failed. Missing or empty responses for IDs: {missing_ids}.")
                if attempt < max_retries - 1:
                    logger.warning("         Retrying entire batch...")
                    time.sleep(retry_delay)
                # Continue to the next attempt in the loop

        except Exception as e:
            logger.error(f"      -> API Error during {job_name} batch with model '{model_to_use}': {e}", exc_info=False)
            if attempt < max_retries - 1:
                logger.warning("         Retrying entire batch due to API error...")
                time.sleep(retry_delay)
    
    logger.error(f"LLM batch failed for {job_name} after {max_retries} attempts. Halting pipeline.")
    return None


def _parse_singleline_llm_response(raw_text: str) -> Dict[str, str]:
    """Parses simple 'ID: response' formats."""
    parsed = {}
    line_regex = re.compile(r"^\s*([^:]+):\s*(.*)$")
    for line in raw_text.splitlines():
        match = line_regex.match(line)
        if match:
            parsed[match.group(1).strip()] = match.group(2).strip()
    return parsed

#
def _parse_structured_llm_response(raw_text: str, expected_ids: List[str]) -> Dict[str, str]:
    """
    Parses a response that contains blocks for multiple IDs, extracting only
    the content within the MAPPINGS section for each ID.
    """
    parsed = {}
    
    # Create a regex to find any of the expected IDs at the start of a block
    id_pattern = "|".join(re.escape(id) for id in expected_ids)
    # Split the entire response text into chunks, one for each ID
    # The (?=...) is a positive lookahead that keeps the delimiter (the next ID)
    block_splitter = re.compile(rf"(?=^\s*(?:{id_pattern})\s*:)", re.MULTILINE)
    
    blocks = [b for b in block_splitter.split(raw_text) if b.strip()]

    for block in blocks:
        lines = block.strip().splitlines()
        if not lines:
            continue
        
        # The first line should contain the ID
        first_line_parts = lines[0].split(':', 1)
        if len(first_line_parts) != 2:
            continue
        
        current_id = first_line_parts[0].strip()
        if current_id not in expected_ids:
            continue
            
        # State machine to parse the content within this block
        collecting = False
        buffer = []
        for line in lines[1:]: # Start from the line after the ID
            line_upper_stripped = line.strip().upper()
            
            if line_upper_stripped.startswith("MAPPINGS:"):
                collecting = True
                # If MAPPINGS: has content on the same line, grab it
                if len(line.strip()) > len("MAPPINGS:"):
                    buffer.append(line.split(":", 1)[1].strip())
                continue
            
            if line_upper_stripped.startswith("VALIDATION:"):
                collecting = False
                # Stop processing this block once validation is reached
                break 
            
            if collecting:
                buffer.append(line)
        
        if buffer:
            parsed[current_id] = "\n".join(buffer).strip()

    return parsed