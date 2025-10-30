# llm2books/llm_logger.py
import logging
from pathlib import Path
from typing import Optional, Dict

logger = logging.getLogger("pipeline")

class LLMLogger:
    def __init__(self, log_dir: Path):
        self.log_dir = log_dir
        self.log_dir.mkdir(parents=True, exist_ok=True)
        self._system_prompts_logged = set()
    
    def log_validation_failure(self, job_name: str, original_text: str, corrupted_output: str, reason: str):
        """Logs segmentation validation failures to a dedicated file."""
        log_file = self.log_dir / f"{job_name}.validation_fails.log"
        try:
            with open(log_file, "a", encoding="utf-8") as f:
                f.write("=" * 80 + "\n")
                f.write(f"VALIDATION FAILED: {reason}\n")
                f.write("-" * 80 + "\n")
                f.write("Original Text:\n")
                f.write(original_text + "\n")
                f.write("-" * 80 + "\n")
                f.write("Corrupted LLM Output:\n")
                f.write(corrupted_output + "\n")
                f.write("=" * 80 + "\n\n")
        except IOError as e:
            logger.warning(f"Could not write to validation log file {log_file.name}: {e}")
    def log_batch(self, job_name: str, batch_num: int, system_prompt: str, user_prompt: str, response: str, usage_stats: Optional[Dict] = None):
        log_file = self.log_dir / f"{job_name}.log"
        job_and_batch = f"{job_name} - Batch #{batch_num}"
        
        try:
            with open(log_file, "a", encoding="utf-8") as f:
                # Log the system prompt only once per job
                if job_name not in self._system_prompts_logged:
                    f.write("=" * 80 + "\n")
                    f.write(f"SYSTEM PROMPT for Job: {job_name}\n")
                    f.write("=" * 80 + "\n")
                    f.write(system_prompt + "\n\n")
                    self._system_prompts_logged.add(job_name)
                
                f.write("-" * 80 + "\n")
                f.write(f"USER PROMPT for: {job_and_batch}\n")
                f.write("-" * 80 + "\n")
                f.write(user_prompt + "\n\n")
                
                # --- START: NEW USAGE STATS LOGGING ---
                if usage_stats:
                    f.write("-" * 80 + "\n")
                    f.write(f"USAGE STATS for: {job_and_batch}\n")
                    f.write("-" * 80 + "\n")
                    
                    cache_status = usage_stats.get('cache_status', 'Unknown')
                    f.write(f"Cache Status: {cache_status}\n")
                    
                    input_tokens = usage_stats.get('input_tokens', 'N/A')
                    f.write(f"Input Tokens: {input_tokens}\n")
                    
                    output_tokens = usage_stats.get('output_tokens', 'N/A')
                    f.write(f"Output Tokens (incl. thinking): {output_tokens}\n\n")
                # --- END: NEW USAGE STATS LOGGING ---

                f.write("-" * 80 + "\n")
                f.write(f"LLM RESPONSE for: {job_and_batch}\n")
                f.write("-" * 80 + "\n")
                f.write(response + "\n\n")

        except IOError as e:
            logger.warning(f"Could not write to LLM log file {log_file.name}: {e}")