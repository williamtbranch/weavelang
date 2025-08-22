# In llm2books/llm_utils.py

import logging
import re
import time
from typing import Dict, List, Optional, Any

from .llm_logger import LLMLogger

# Get the logger at the module level
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
    max_retries = stage_config.get("max_api_retries", 3)
    retry_delay = stage_config.get("retry_delay", 7)
    batch_size = stage_config.get("batch_size_in_items", 10)

    primary_model_key = stage_config.get("primary_model")
    fallback_model_key = stage_config.get("fallback_model")
    primary_model_name = models_config.get(primary_model_key, {}).get("name")
    fallback_model_name = models_config.get(fallback_model_key, {}).get("name") if fallback_model_key else None
    
    if not primary_model_name:
        logger.error(f"Model config error for stage '{job_name}': Could not find model details for key '{primary_model_key}'")
        return None

    all_results = []
    batch_num = 0

    for i in range(0, len(items_to_process), batch_size):
        batch_num += 1
        batch = items_to_process[i:i + batch_size]
        prompt_ids = [item['id'] for item in batch]
        accumulated_responses = {}
        current_user_prompt = "\n".join([f"{item['id']}:\n{item['text']}" for item in batch])
        
        for attempt in range(max_retries):
            model_to_use = primary_model_name
            if attempt == max_retries - 1 and fallback_model_name and fallback_model_name != primary_model_name:
                model_to_use = fallback_model_name
                logger.info(f"    -> Switching to fallback model for final attempt.")

            logger.info(f"    -> We are Running {job_name} LLM batch {batch_num} (Attempt {attempt + 1}/{max_retries}) using model '{model_to_use}'...")
            
            raw_response = ""
            try:
                message = llm_client.messages.create(
                    model=model_to_use, system=system_prompt, 
                    messages=[{"role": "user", "content": current_user_prompt}], 
                    max_tokens=4096, temperature=0.1
                )
                raw_response = message.content[0].text if message.content else ""
                llm_logger.log_batch(job_name, batch_num, system_prompt, current_user_prompt, raw_response)
                
                if parser_type == 'single_line':
                    parsed_response = _parse_singleline_llm_response(raw_response)
                else:
                    parsed_response = _parse_llm_mappings_by_id(raw_response)
                
                if len(parsed_response) == 1 and len(batch) > 0:
                    first_item_id = batch[0]['id']
                    if first_item_id not in parsed_response:
                        logger.warning(f"      -> LLM response was not prefixed. Assuming it belongs to first item ID: {first_item_id}")
                        single_response_content = list(parsed_response.values())[0]
                        parsed_response = {first_item_id: single_response_content}
                
                for pid, text in parsed_response.items():
                    if pid in prompt_ids and pid not in accumulated_responses:
                        accumulated_responses[pid] = text
                
                missing_ids = [pid for pid in prompt_ids if pid not in accumulated_responses]

                if not missing_ids:
                    for item in batch:
                        item['llm_response'] = accumulated_responses[item['id']]
                    all_results.extend(batch)
                    break
                else:
                    logger.warning(f"      -> {job_name} batch still missing IDs after attempt {attempt + 1}: {missing_ids}. Retrying...")
                    retry_prompt_header = "You did not provide a valid response for all requested IDs. Provide output ONLY for the following missing IDs.\n"
                    missing_items_text = "\n".join([f"{item['id']}:\n{item['text']}" for item in batch if item['id'] in missing_ids])
                    current_user_prompt = f"{retry_prompt_header}{missing_items_text}"

            except Exception as e:
                logger.error(f"      -> API Error during {job_name} batch with model '{model_to_use}': {e}", exc_info=True)
                if raw_response: 
                    llm_logger.log_batch(job_name, batch_num, system_prompt, current_user_prompt, f"FAILED_RESPONSE: {raw_response}\nERROR: {e}")
            
            time.sleep(retry_delay)
        
        else:
            logger.error(f"LLM batch failed for {job_name} after {max_retries} attempts. Halting pipeline.")
            return None
            
    return all_results

def _parse_singleline_llm_response(raw_text: str) -> Dict[str, str]:
    parsed = {}
    line_regex = re.compile(r"^([^:]+):\s*(.*)$")
    for line in raw_text.splitlines():
        match = line_regex.match(line)
        if match:
            parsed[match.group(1).strip()] = match.group(2).strip().strip('"')
    return parsed

# --- INSTRUMENTED PARSER WITH DEBUG LOGGING ---
def _parse_llm_mappings_by_id(raw_text: str) -> Dict[str, str]:
    """
    Parses the LLM response using a state machine with detailed logging and
    a corrected, more flexible regex for sentence and segment IDs.
    """
    logger.debug("--- [PARSER START] ---")
    parsed = {}
    current_id = None
    collecting_mappings = False
    
    # --- THIS IS THE FIX ---
    # The regex now matches 'S' followed by digits, optionally followed
    # by an underscore and more alphanumeric characters (like another 'S' and digits).
    id_marker_regex = re.compile(r"^\s*(S\d+(?:_[A-Za-z0-9_]+)?):")

    for i, line in enumerate(raw_text.splitlines()):
        logger.debug(f"  [PARSER] Line {i+1}: '{line[:80]}'")
        
        match = id_marker_regex.match(line)
        if match:
            new_id = match.group(1)
            logger.debug(f"    -> Found new ID marker: '{new_id}'. Resetting state.")
            current_id = new_id
            parsed.setdefault(current_id, [])
            collecting_mappings = False
            continue

        if current_id:
            line_upper = line.strip().upper()

            if line_upper.startswith("MAPPINGS:"):
                logger.debug(f"    -> Entering MAPPINGS section for '{current_id}'.")
                collecting_mappings = True
                continue
            
            if line_upper.startswith("VALIDATION:") or line_upper.startswith("SPANISH_TRANSLATION:"):
                if collecting_mappings:
                    logger.debug(f"    -> Exiting MAPPINGS section for '{current_id}'.")
                collecting_mappings = False
                continue

            if collecting_mappings and line.strip():
                logger.debug(f"    -> Collecting line for '{current_id}': '{line.strip()}'")
                parsed[current_id].append(line)
        else:
            logger.debug("    -> Skipping line (no active ID).")

    final_parsed = {
        key: "\n".join(lines) for key, lines in parsed.items() if lines
    }
    
    if not final_parsed and raw_text.strip():
        logger.debug("  [PARSER] No ID markers found. Treating as one anonymous block.")
        in_mappings = False
        mapping_lines = []
        for line in raw_text.splitlines():
            if line.strip().upper().startswith("MAPPINGS:"): in_mappings = True; continue
            if line.strip().upper().startswith("VALIDATION:"): break
            if in_mappings: mapping_lines.append(line)
        if mapping_lines:
            anonymous_content = "\n".join(mapping_lines)
            logger.debug(f"  [PARSER] Found anonymous mapping content:\n{anonymous_content[:200]}...")
            return {"ANONYMOUS_BLOCK": anonymous_content}

    logger.debug(f"--- [PARSER END] Returning {len(final_parsed)} parsed items. Keys: {list(final_parsed.keys())} ---")
    return final_parsed