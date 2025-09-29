import logging
from pathlib import Path
from typing import Dict
import re

from .llm_logger import LLMLogger

logger = logging.getLogger("pipeline")

def load_manual_overrides(job_name: str, llm_logger: LLMLogger) -> Dict[str, str]:
    """
    Scans an LLM log file for one or more %%MANUAL_FIX%%...%%END_MANUAL_FIX%% blocks
    and parses them. If an ID is defined in multiple blocks, the last one wins.
    """
    override_map: Dict[str, str] = {}
    log_file = llm_logger.log_dir / f"{job_name}.log"
    
    if not log_file.exists():
        return override_map

    try:
        content = log_file.read_text(encoding="utf-8")
        if "%%MANUAL_FIX%%" not in content:
            return override_map

        fix_blocks = re.findall(r"%%MANUAL_FIX%%(.*?)%%END_MANUAL_FIX%%", content, re.DOTALL)
        
        if not fix_blocks:
            if "%%END_MANUAL_FIX%%" not in content:
                 logger.warning(f"Found legacy %%MANUAL_FIX%% block without an end marker in '{log_file.name}'. Please add %%END_MANUAL_FIX%%.")
                 fix_blocks = [content.split("%%MANUAL_FIX%%", 1)[1]]
            else:
                 return override_map

        logger.warning(f"  -> Found {len(fix_blocks)} manual fix block(s) in '{log_file.name}'. Applying overrides for job '{job_name}'.")

        for fix_block in fix_blocks:
            # --- START OF NEW, ROBUST PARSER ---
            current_id = None
            buffer = []
            
            for line in fix_block.splitlines():
                # Check if the line is a new ID definition
                match = re.match(r"^\s*([A-Z0-9_]+)\s*:", line)
                if match:
                    # If we were processing a previous ID, save its buffered content
                    if current_id and buffer:
                        content_str = "\n".join(buffer).strip()
                        if "MAPPINGS:" in content_str:
                             # Extract only the content after MAPPINGS: and before VALIDATION:
                            mappings_part = content_str.split("MAPPINGS:", 1)[1]
                            if "VALIDATION:" in mappings_part:
                                mappings_content = mappings_part.split("VALIDATION:", 1)[0].strip()
                            else:
                                mappings_content = mappings_part.strip()
                            
                            if mappings_content:
                                override_map[current_id] = mappings_content
                                logger.info(f"     -> Loaded manual fix for ID: {current_id}")

                    # Start processing the new ID
                    current_id = match.group(1).strip()
                    buffer = [line.split(":", 1)[1].strip()] # Start buffer with content after colon
                elif current_id:
                    # If we are inside an ID block, append the line
                    buffer.append(line)
            
            # Save the very last ID block in the file
            if current_id and buffer:
                content_str = "\n".join(buffer).strip()
                if "MAPPINGS:" in content_str:
                    mappings_part = content_str.split("MAPPINGS:", 1)[1]
                    if "VALIDATION:" in mappings_part:
                        mappings_content = mappings_part.split("VALIDATION:", 1)[0].strip()
                    else:
                        mappings_content = mappings_part.strip()
                    
                    if mappings_content:
                        override_map[current_id] = mappings_content
                        logger.info(f"     -> Loaded manual fix for ID: {current_id}")
            # --- END OF NEW, ROBUST PARSER ---
        
    except Exception as e:
        logger.error(f"Could not parse manual override block from {log_file.name}: {e}")

    return override_map