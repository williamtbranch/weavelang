# llm2books/llm_utils.py
import logging
import re
import time
from typing import Dict, List, Optional

from .llm_logger import LLMLogger

logger = logging.getLogger("pipeline")

#
def run_llm_batch_job(llm_client: any, job_name: str, system_prompt: str, items_to_process: List[Dict], llm_logger: LLMLogger, parser_type: str) -> Optional[List[Dict]]:
    BATCH_SIZE, MAX_RETRIES, RETRY_DELAY = 10, 3, 5
    all_results = []
    batch_num = 0

    for i in range(0, len(items_to_process), BATCH_SIZE):
        batch_num += 1
        batch = items_to_process[i:i + BATCH_SIZE]
        
        prompt_ids = [item['id'] for item in batch]
        
        # --- NEW: ACCUMULATION LOGIC ---
        # This dictionary will store the successful results from all retry attempts.
        accumulated_responses = {}
        # --- END NEW ---

        # Start with the full original prompt
        current_user_prompt = "\n".join([f"{item['id']}: {item['text']}" for item in batch])
        
        for attempt in range(MAX_RETRIES):
            logger.info(f"    -> Running {job_name} LLM batch {batch_num} (Attempt {attempt + 1}/{MAX_RETRIES})...")
            
            raw_response = ""
            try:
                message = llm_client.messages.create(
                    model="claude-3-haiku-20240307", 
                    system=system_prompt, 
                    messages=[{"role": "user", "content": current_user_prompt}], 
                    max_tokens=4096
                )
                raw_response = message.content[0].text
                llm_logger.log_batch(job_name, batch_num, system_prompt, current_user_prompt, raw_response)
                
                if parser_type == 'single_line':
                    parsed_response = _parse_singleline_llm_response(raw_response)
                else:
                    parsed_response = _parse_multiline_llm_response(raw_response)
                
                # --- NEW: ACCUMULATION LOGIC ---
                # Add any newly received, valid responses to our collection
                for pid, text in parsed_response.items():
                    if pid in prompt_ids and pid not in accumulated_responses:
                        accumulated_responses[pid] = text
                # --- END NEW ---

                # Now, check if our accumulated collection is complete
                missing_ids = [pid for pid in prompt_ids if pid not in accumulated_responses]

                if not missing_ids:
                    # Success! All items have been collected.
                    for item in batch:
                        item['llm_response'] = accumulated_responses[item['id']]
                    all_results.extend(batch)
                    break # Exit the retry loop for this batch
                else:
                    # Failure. Prepare a focused retry prompt for ONLY the truly missing items.
                    logger.warning(f"      -> {job_name} batch still missing IDs after attempt {attempt + 1}: {missing_ids}. Retrying...")
                    
                    retry_prompt_header = (
                        "You did not provide a response for all of the requested IDs in your previous attempt. "
                        "Please provide the output ONLY for the following missing IDs.\n"
                    )
                    
                    missing_items_text = "\n".join([
                        f"{item['id']}: {item['text']}" 
                        for item in batch if item['id'] in missing_ids
                    ])
                    
                    current_user_prompt = f"{retry_prompt_header}{missing_items_text}"

            except Exception as e:
                logger.error(f"      -> API Error during {job_name} batch: {e}. Retrying...")
                if raw_response: 
                    llm_logger.log_batch(job_name, batch_num, system_prompt, current_user_prompt, f"FAILED_RESPONSE: {raw_response}\nERROR: {e}")
            
            time.sleep(RETRY_DELAY)
        else:
            # This 'else' block runs only if the 'for' loop completes without a 'break'
            logger.error(f"      -> {job_name} batch failed after {MAX_RETRIES} retries. Aborting job.")
            return None
            
    return all_results

def _parse_singleline_llm_response(raw_text: str) -> Dict[str, str]:
    parsed = {}
    line_regex = re.compile(r"^([^:]+):\s*(.*)$")
    for line in raw_text.splitlines():
        match = line_regex.match(line)
        if match:
            parsed[match.group(1).strip()] = match.group(2).strip()
    return parsed

#
def _parse_multiline_llm_response(raw_text: str) -> Dict[str, str]:
    """
    Parses a multi-line LLM response where an ID may be on its own line
    followed by the content.
    """
    parsed = {}
    current_id = None
    current_lines = []
    
    # Regex to find an ID at the start of a line, ending with an optional colon.
    id_regex = re.compile(r"^\s*([A-Za-z0-9_]+):?\s*$")

    for line in raw_text.strip().splitlines():
        match = id_regex.match(line)
        
        # Check if the line ONLY contains an ID (and maybe a colon)
        if match and len(line.strip().replace(":", "")) == len(match.group(1)):
            # Finalize the previous block if it exists
            if current_id:
                parsed[current_id] = "\n".join(current_lines).strip()
            
            # Start a new block
            current_id = match.group(1).strip()
            current_lines = []
        elif current_id:
            # This is a content line for the current block
            current_lines.append(line)

    # After the loop, finalize the last block
    if current_id:
        parsed[current_id] = "\n".join(current_lines).strip()
        
    return parsed