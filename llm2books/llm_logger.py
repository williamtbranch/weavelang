# llm2books/llm_logger.py
import logging
from pathlib import Path

logger = logging.getLogger("pipeline")

class LLMLogger:
    def __init__(self, log_dir: Path):
        self.log_dir = log_dir
        self.log_dir.mkdir(parents=True, exist_ok=True)
        self._system_prompts_logged = set()

    def log_batch(self, job_name: str, batch_num: int, system_prompt: str, user_prompt: str, response: str):
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
                
                f.write("-" * 80 + "\n")
                f.write(f"LLM RESPONSE for: {job_and_batch}\n")
                f.write("-" * 80 + "\n")
                f.write(response + "\n\n")

        except IOError as e:
            logger.warning(f"Could not write to LLM log file {log_file.name}: {e}")