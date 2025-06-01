# -----------------------------------------------------------------------------
# book_to_audio.py
# Script Version: 1.1.0 (Added metadata for repair mode)
# ... (SDK Context and other comments as before) ...
# -----------------------------------------------------------------------------

import asyncio
import argparse
from pathlib import Path
import os
import logging
import wave
import time
import importlib.metadata # For getting package version
import json # For metadata

SCRIPT_VERSION = "1.1.0"

# --- Enhanced Debug Logging Setup ---
# (set_library_log_levels function as before)
logging.basicConfig(format='%(asctime)s - %(levelname)s - %(name)s - %(message)s')

def set_library_log_levels(script_log_level_str: str):
    if script_log_level_str.upper() == "DEBUG":
        logging.getLogger("httpx").setLevel(logging.DEBUG)
        logging.getLogger("google.generativeai._base_client").setLevel(logging.DEBUG) # Old SDK path
        logging.getLogger("google.genai._base_client").setLevel(logging.DEBUG) # New SDK path attempt
        logging.getLogger("google.genai.client").setLevel(logging.DEBUG)      # New SDK path attempt
        logging.getLogger("google.api_core.retry").setLevel(logging.DEBUG)
        logging.getLogger("google.api_core.grpc_helpers").setLevel(logging.DEBUG)
        logging.getLogger("google.auth.transport.requests").setLevel(logging.DEBUG)
    else:
        logging.getLogger("httpx").setLevel(logging.WARNING)
        logging.getLogger("google.generativeai._base_client").setLevel(logging.WARNING)
        logging.getLogger("google.genai._base_client").setLevel(logging.WARNING)
        logging.getLogger("google.genai.client").setLevel(logging.WARNING)
        logging.getLogger("google.api_core.retry").setLevel(logging.WARNING)
        logging.getLogger("google.api_core.grpc_helpers").setLevel(logging.WARNING)
        logging.getLogger("google.auth.transport.requests").setLevel(logging.WARNING)


from google import genai as google_genai
from google.genai import types as genai_types
from google.api_core import exceptions as api_core_exceptions
from dotenv import load_dotenv
from pydub import AudioSegment
import tomllib

# --- Configuration --- (Defaults as before)
DEFAULT_MODEL_NAME = "models/gemini-2.5-pro-preview-tts" # Updated default
DEFAULT_VOICE_NAME = "Schedar" # Your preferred voice
DEFAULT_TTS_PROMPT_PREFIX = "You have a Mexican Spanish accent. Narrate the following text in a clear, even voice, suitable for an audiobook. You are telling a story. Be engaging:"
DEFAULT_CHUNK_MAX_CHARS = 750 # Adjusted based on 1-2 min chunk discussion
DEFAULT_CONCURRENT_REQUESTS = 1 # Start with 1 for stability
DEFAULT_API_RETRIES = 10
DEFAULT_RETRY_DELAY = 15 # seconds
DEFAULT_OUTPUT_AUDIO_FORMAT = "wav"
TEMP_DIR_NAME = "_tts_temp_chunks"
METADATA_FILENAME = "_metadata.json"
REQUEST_TIMEOUT_SECONDS = 1800

PCM_CHANNELS = 1
PCM_FRAME_RATE = 24000
PCM_SAMPLE_WIDTH = 2

# --- Helper Functions ---
# (load_project_config, save_raw_pcm_to_wav, chunk_text - as before)
def load_project_config(tool_root_dir: Path) -> dict:
    config_file_path = tool_root_dir / "config.toml"
    if not config_file_path.exists():
        logging.error(f"Tool config.toml not found at {config_file_path}")
        return {}
    try:
        with open(config_file_path, "rb") as f:
            data = tomllib.load(f)
        return data
    except Exception as e:
        logging.error(f"Error loading tool config.toml: {e}")
        return {}

def save_raw_pcm_to_wav(filename: Path, pcm_data: bytes, channels: int, rate: int, sample_width: int):
    with wave.open(str(filename), "wb") as wf:
        wf.setnchannels(channels)
        wf.setsampwidth(sample_width)
        wf.setframerate(rate)
        wf.writeframes(pcm_data)
    logging.debug(f"Raw PCM data saved to WAV: {filename}")

def chunk_text(full_text: str, max_chars: int) -> list[str]:
    chunks = []
    current_pos = 0
    text_len = len(full_text)
    while current_pos < text_len:
        end_pos = min(current_pos + max_chars, text_len)
        chunk = full_text[current_pos:end_pos]
        if end_pos < text_len: # Try to break at paragraph or sentence if not the last chunk
            para_break = chunk.rfind('\n\n')
            if para_break != -1 and para_break > max_chars // 3 : # Prefer para break if reasonably sized
                chunk = chunk[:para_break + 2]
                end_pos = current_pos + len(chunk)
            else:
                sentence_break = -1
                for sb_marker in ['. ', '! ', '? ', '." ', '!" ', '?" ','.\n', '!\n', '?\n']: # Added newline sentence breaks
                    s_idx = chunk.rfind(sb_marker)
                    if s_idx != -1:
                        potential_break = s_idx + len(sb_marker)
                        if potential_break > sentence_break:
                            sentence_break = potential_break
                
                if sentence_break != -1 and sentence_break > max_chars // 3:
                    chunk = chunk[:sentence_break]
                    end_pos = current_pos + len(chunk)
        final_chunk = chunk.strip()
        if final_chunk:
            chunks.append(final_chunk)
        current_pos = end_pos
    
    # Ensure the very last bit of text isn't missed if not perfectly chunked
    if chunks:
        concatenated_chunks_len = sum(len(c) for c in chunks) # Rough check, spaces between not accounted
        # A more accurate check would be to re-join and compare with original, but this is simpler
        # This simplistic check might not be perfect for exact reconstruction of original text length.
        # The core idea is that all *content* should be chunked.
    return chunks


# (generate_audio_chunk_async - largely as before, with robust error handling)
async def generate_audio_chunk_async(
    client: google_genai.Client,
    text_chunk: str,
    chunk_index: int, # This is the 0-based index from the original full list of chunks
    args: argparse.Namespace, # Now contains the effective args (potentially from metadata in repair)
    semaphore: asyncio.Semaphore,
    temp_chunk_dir: Path
) -> Path | None:
    async with semaphore:
        content_for_api = f"{args.tts_prompt_prefix}\n\n{text_chunk}"
        logging.info(f"Requesting TTS for chunk {chunk_index + 1} (Original Index) (prompt+text length: {len(content_for_api)} chars)...")
        last_exception = None
        for attempt in range(args.max_api_retries):
            try:
                if not text_chunk.strip():
                    logging.warning(f"Chunk {chunk_index + 1} (Original Index) is effectively empty. Creating 100ms silence.")
                    silence = AudioSegment.silent(duration=100, frame_rate=PCM_FRAME_RATE)
                    # Always use the original chunk_index for consistent naming
                    temp_file_path = temp_chunk_dir / f"temp_chunk_{chunk_index:04d}_silence.wav"
                    silence.export(str(temp_file_path), format="wav", parameters=["-ac", str(PCM_CHANNELS), "-ar", str(PCM_FRAME_RATE)])
                    return temp_file_path

                speech_config_dict = {"voice_config": {"prebuilt_voice_config": {"voice_name": args.voice_name}}}
                api_config = genai_types.GenerateContentConfig(
                    response_modalities=["AUDIO"],
                    speech_config=speech_config_dict
                )
                logging.debug(f"Chunk {chunk_index + 1} (Attempt {attempt + 1}/{args.max_api_retries}): Preparing to call generate_content. Applying asyncio.wait_for with timeout {REQUEST_TIMEOUT_SECONDS}s.")
                api_response = await asyncio.wait_for(
                    client.aio.models.generate_content(
                        model=args.model_name, contents=content_for_api, config=api_config
                    ), timeout=REQUEST_TIMEOUT_SECONDS
                )
                logging.debug(f"Chunk {chunk_index + 1} (Attempt {attempt + 1}/{args.max_api_retries}): Call to generate_content completed within timeout.")

                if api_response.candidates and api_response.candidates[0].content and \
                   api_response.candidates[0].content.parts and \
                   api_response.candidates[0].content.parts[0].inline_data and \
                   api_response.candidates[0].content.parts[0].inline_data.data:
                    audio_data_bytes = api_response.candidates[0].content.parts[0].inline_data.data
                    temp_file_path = temp_chunk_dir / f"temp_chunk_{chunk_index:04d}.wav" # Use original index
                    save_raw_pcm_to_wav(temp_file_path, audio_data_bytes, PCM_CHANNELS, PCM_FRAME_RATE, PCM_SAMPLE_WIDTH)
                    logging.info(f"Chunk {chunk_index + 1} (Original Index) successfully converted and saved to {temp_file_path}.")
                    return temp_file_path
                else:
                    finish_reason_str = "N/A"
                    if api_response.candidates and api_response.candidates[0].finish_reason:
                        finish_reason_str = str(api_response.candidates[0].finish_reason)
                    error_message = f"No audio data in API response (200 OK but FinishReason: {finish_reason_str})."
                    logging.error(f"Chunk {chunk_index + 1} (Original Index) (Attempt {attempt + 1}/{args.max_api_retries}) - {error_message}")
                    logging.error(f"Chunk {chunk_index + 1} (Original Index) (Attempt {attempt + 1}/{args.max_api_retries}) - Failing text content was (text_chunk):\n-------\n{text_chunk}\n-------")
                    logging.debug(f"Full API Response for errored chunk {chunk_index + 1}: {api_response}")
                    last_exception = RuntimeError(error_message)
            except asyncio.TimeoutError as e:
                logging.error(f"Chunk {chunk_index + 1} (Original Index) - Application-level timeout via asyncio.wait_for after {REQUEST_TIMEOUT_SECONDS}s (Attempt {attempt + 1}/{args.max_api_retries}).")
                last_exception = e
            except api_core_exceptions.DeadlineExceeded as e:
                logging.error(f"Chunk {chunk_index + 1} (Original Index) - SDK DeadlineExceeded (Attempt {attempt + 1}/{args.max_api_retries}): {e}", exc_info=True if args.log_level.upper() == "DEBUG" else False)
                last_exception = e
            except TypeError as e:
                sdk_version_str = "unknown"; gtag_version = "unknown"
                try: sdk_version_str = importlib.metadata.version("google-genai")
                except: pass
                logging.error(f"Chunk {chunk_index + 1} (Original Index) - Critical TypeError (Attempt {attempt+1}): {e}. `google-genai` SDK version: {sdk_version_str}. Config/arguments might be incorrect.", exc_info=True)
                return None
            except AttributeError as e:
                logging.error(f"Chunk {chunk_index + 1} (Original Index) - AttributeError (Attempt {attempt + 1}/{args.max_api_retries}): {e}", exc_info=True)
                last_exception = e
            except Exception as e:
                logging.error(f"Chunk {chunk_index + 1} (Original Index) - General Error (Attempt {attempt + 1}/{args.max_api_retries}): {e}", exc_info=True)
                last_exception = e
            
            if attempt + 1 < args.max_api_retries:
                delay = args.retry_delay
                error_str = str(last_exception).lower() if last_exception else ""
                if "429" in error_str or "resource_exhausted" in error_str or "rate limit" in error_str or "unavailable" in error_str or "503" in error_str:
                    delay = args.retry_delay * (2 ** attempt)
                    logging.info(f"Rate limit or temporary server issue likely hit for chunk {chunk_index + 1}. Retrying in {delay} seconds...")
                else:
                    logging.info(f"Retrying chunk {chunk_index + 1} (Attempt {attempt + 2}/{args.max_api_retries}) after error. Waiting {delay} seconds...")
                await asyncio.sleep(delay)
        logging.error(f"Chunk {chunk_index + 1} (Original Index) failed after {args.max_api_retries} retries. Last error: {last_exception}")
        return None

async def process_book_to_audio_async(
    text_chunks_from_file: list[str], # Original chunks from text file
    client: google_genai.Client,
    output_file_path: Path,
    args: argparse.Namespace, # These are the current run's command-line args
    effective_args: argparse.Namespace # These are args to USE (from metadata in repair mode)
):
    semaphore = asyncio.Semaphore(effective_args.concurrent_requests) # Use effective_args for concurrency
    temp_chunk_dir = output_file_path.parent / TEMP_DIR_NAME / output_file_path.stem
    temp_chunk_dir.mkdir(parents=True, exist_ok=True)
    metadata_file_path = temp_chunk_dir / METADATA_FILENAME
    
    logging.info(f"Temporary audio chunks for '{output_file_path.name}' will be stored in: {temp_chunk_dir}")

    # In repair mode, 'text_chunks_from_file' is based on effective_args.chunk_max_chars from metadata
    # In normal mode, it's from current args.chunk_max_chars
    total_expected_chunks_count = len(text_chunks_from_file) 
    logging.info(f"Total expected chunks based on current/metadata settings: {total_expected_chunks_count}")


    tasks_to_generate = [] # List of (original_index, text_content)
    
    if effective_args.repair_mode:
        logging.info("--- REPAIR MODE ENABLED ---")
        # Metadata should have already been loaded and effective_args populated
        for i, text_content in enumerate(text_chunks_from_file):
            if not text_content.strip(): # This original chunk was empty/whitespace
                logging.debug(f"Repair mode: Original chunk {i+1} was empty, considered processed.")
                continue # Don't try to generate, assume silence file logic handles it or it's skipped

            expected_audio_file = temp_chunk_dir / f"temp_chunk_{i:04d}.wav"
            expected_silence_file = temp_chunk_dir / f"temp_chunk_{i:04d}_silence.wav"
            
            # Check for existing valid audio or silence file
            is_valid_existing = False
            if expected_audio_file.exists() and expected_audio_file.stat().st_size > 44:
                try: AudioSegment.from_file(expected_audio_file) # More robust check
                except: pass
                else: is_valid_existing = True; logging.info(f"Repair mode: Chunk {i+1} (audio) found valid: {expected_audio_file}")
            
            if not is_valid_existing and expected_silence_file.exists() and expected_silence_file.stat().st_size > 44:
                try: AudioSegment.from_file(expected_silence_file)
                except: pass
                else: is_valid_existing = True; logging.info(f"Repair mode: Chunk {i+1} (silence) found valid: {expected_silence_file}")

            if not is_valid_existing:
                logging.info(f"Repair mode: Chunk {i+1} needs generation.")
                tasks_to_generate.append((i, text_content)) 
    else: # Normal mode
        logging.info("--- NORMAL MODE ---")
        # Save metadata for this normal run
        metadata_to_save = {
            "script_version": SCRIPT_VERSION,
            "input_filename": effective_args.input_filename, # From original args
            "model_name": effective_args.model_name,
            "voice_name": effective_args.voice_name,
            "tts_prompt_prefix": effective_args.tts_prompt_prefix,
            "chunk_max_chars": effective_args.chunk_max_chars,
            "total_expected_chunks": total_expected_chunks_count
        }
        try:
            with open(metadata_file_path, 'w') as mf:
                json.dump(metadata_to_save, mf, indent=4)
            logging.info(f"Saved metadata to {metadata_file_path}")
        except IOError as e:
            logging.error(f"Could not save metadata file {metadata_file_path}: {e}")
            # Decide if this is a fatal error for normal mode (probably not, but good to warn)

        for i, text_content in enumerate(text_chunks_from_file):
            if not text_content.strip(): # Skip generating for originally empty text
                logging.info(f"Normal mode: Original chunk {i+1} is empty, will be handled by silence generation if needed or skipped.")
                # generate_audio_chunk_async will create a silence file for it
                tasks_to_generate.append((i, text_content)) # Still add, let generate_audio_chunk_async make silence
                continue
            tasks_to_generate.append((i, text_content))

    # --- TTS Generation for required chunks ---
    generation_tasks_for_asyncio = []
    if tasks_to_generate:
        logging.info(f"Preparing {len(tasks_to_generate)} chunks for TTS API calls.")
        for original_idx, text_content_for_tts in tasks_to_generate:
            generation_tasks_for_asyncio.append(
                generate_audio_chunk_async(client, text_content_for_tts, original_idx, effective_args, semaphore, temp_chunk_dir)
            )
        
        if generation_tasks_for_asyncio:
            logging.info(f"Submitting {len(generation_tasks_for_asyncio)} tasks to asyncio.gather...")
            # Results are paths to generated files or None/Exceptions
            results_from_gather = await asyncio.gather(*generation_tasks_for_asyncio, return_exceptions=True)
            
            # Log any exceptions from asyncio.gather itself (should be rare if generate_audio_chunk_async handles its own)
            for i, res in enumerate(results_from_gather):
                if isinstance(res, Exception):
                    # This 'i' is the index in results_from_gather, need to map to original_idx if possible
                    # original_idx_for_this_task = tasks_to_generate[i][0]
                    logging.error(f"A generation task (for original chunk approx. {tasks_to_generate[i][0]+1}) resulted in an unhandled exception from asyncio.gather: {res}", exc_info=res if effective_args.log_level.upper() == "DEBUG" else False)
    else:
        logging.info("No chunks required TTS generation in this run.")

    # --- Concatenation Stage ---
    all_final_chunk_paths = []
    all_expected_chunks_accounted_for = True
    actual_non_empty_input_chunks_count = 0

    for i in range(total_expected_chunks_count):
        # Check if the original text_chunks_from_file[i] was non-empty
        if not text_chunks_from_file[i].strip():
            # If original text was empty, we might have a silence file or nothing if generate_audio_chunk_async skips fully
            # For now, let's assume generate_audio_chunk_async produces a silence file for empty inputs.
            expected_file = temp_chunk_dir / f"temp_chunk_{i:04d}_silence.wav"
            if expected_file.exists() and expected_file.stat().st_size > 44:
                all_final_chunk_paths.append(expected_file)
                # This was an intentionally empty chunk, so it's "accounted for"
            else:
                # If even silence wasn't created for an empty input chunk, it might be an issue or intended skip.
                logging.debug(f"Original chunk {i+1} was empty, and no corresponding silence file found. Assuming skipped.")
            continue # Move to next expected chunk

        actual_non_empty_input_chunks_count +=1 # This was an actual content chunk
        expected_file = temp_chunk_dir / f"temp_chunk_{i:04d}.wav"
        
        is_valid_file_present = False
        if expected_file.exists() and expected_file.stat().st_size > 44:
            try: AudioSegment.from_file(expected_file); is_valid_file_present = True
            except Exception as e: logging.warning(f"File {expected_file} exists but is not a valid audio file: {e}")
        
        if is_valid_file_present:
            all_final_chunk_paths.append(expected_file)
        else:
            logging.error(f"Final check: Audio for original chunk {i+1} (file: {expected_file}) is missing or invalid before concatenation.")
            all_expected_chunks_accounted_for = False
            # For very long books, you might not want to `break` but try to concatenate what you have.
            # break 

    if not all_final_chunk_paths:
        logging.error("No valid audio chunk files found for concatenation. Final audio not created.")
        return

    def get_chunk_index_from_path(p: Path) -> int:
        try: return int(p.stem.split('_')[-1].replace("_silence", ""))
        except: return -1
    sorted_chunk_paths = sorted(all_final_chunk_paths, key=get_chunk_index_from_path)
    
    if len(sorted_chunk_paths) != actual_non_empty_input_chunks_count and all_expected_chunks_accounted_for:
        # This case implies some originally non-empty chunks did not result in a file,
        # contradicting all_expected_chunks_accounted_for if it was still True.
        # The all_expected_chunks_accounted_for flag should have caught this.
        logging.warning(f"Mismatch: Concatenating {len(sorted_chunk_paths)} audio files, but expected {actual_non_empty_input_chunks_count} from non-empty input chunks. Some files might be missing despite checks.")
    elif not all_expected_chunks_accounted_for:
        logging.warning(f"Concatenating {len(sorted_chunk_paths)} available audio files. Expected {actual_non_empty_input_chunks_count} from non-empty input chunks. Final audio WILL BE INCOMPLETE.")


    logging.info(f"Concatenating {len(sorted_chunk_paths)} audio chunks...")
    combined_audio = AudioSegment.empty()
    for chunk_path in sorted_chunk_paths:
        try:
            logging.debug(f"Loading chunk: {chunk_path}")
            audio_chunk = AudioSegment.from_file(chunk_path)
            combined_audio += audio_chunk
        except Exception as e:
            logging.error(f"Error loading audio chunk {chunk_path} for concatenation: {e}. Skipping this chunk.")
    
    if len(combined_audio) == 0:
        logging.error("Combined audio is empty after attempting concatenation. Final audio file not created.")
        return

    try:
        logging.info(f"Exporting final audio to {output_file_path} (format: {effective_args.output_audio_format})...")
        output_file_path.parent.mkdir(parents=True, exist_ok=True)
        combined_audio.export(str(output_file_path), format=effective_args.output_audio_format)
        logging.info(f"Successfully created audiobook: {output_file_path}")
    except Exception as e:
        logging.error(f"Error exporting final audio: {e}")
        if effective_args.output_audio_format.lower() == "mp3":
            logging.error("Ensure FFmpeg/LibAV is installed for MP3 export.")

    # --- Cleanup Logic ---
    # In repair mode, or if all chunks were present in normal mode, clean up.
    # Otherwise (normal mode with missing chunks), keep temp files.
    if effective_args.repair_mode or all_expected_chunks_accounted_for:
        logging.info(f"Proceeding with cleanup of temporary files from {temp_chunk_dir}...")
        if temp_chunk_dir.exists() and temp_chunk_dir.is_dir():
            for item in temp_chunk_dir.iterdir(): # Remove files and metadata
                try:
                    if item.is_file(): os.remove(item)
                except OSError as e: logging.warning(f"Could not remove temp item {item}: {e}")
            try:
                if not any(temp_chunk_dir.iterdir()): os.rmdir(temp_chunk_dir)
                else: logging.info(f"Temp dir {temp_chunk_dir} not empty after file cleanup, not removed.")
            except OSError as e: logging.warning(f"Could not remove temp dir {temp_chunk_dir}: {e}")
    else:
        logging.warning(f"Not all chunks were successfully generated. Temporary files in {temp_chunk_dir} (including metadata) will be kept for a potential --repair-mode run.")


async def main_async():
    parser = argparse.ArgumentParser(description="Convert a book text file to audio using Google GenAI TTS.")
    parser.add_argument("--input-filename", required=True)
    parser.add_argument("--tool-root-dir", type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument("--api-key", help="Google API Key.")
    parser.add_argument("--model-name", default=DEFAULT_MODEL_NAME)
    parser.add_argument("--voice-name", default=DEFAULT_VOICE_NAME)
    parser.add_argument("--tts-prompt-prefix", default=DEFAULT_TTS_PROMPT_PREFIX)
    parser.add_argument("--chunk-max-chars", type=int, default=DEFAULT_CHUNK_MAX_CHARS)
    parser.add_argument("--concurrent-requests", type=int, default=DEFAULT_CONCURRENT_REQUESTS)
    parser.add_argument("--max-api-retries", type=int, default=DEFAULT_API_RETRIES)
    parser.add_argument("--retry-delay", type=int, default=DEFAULT_RETRY_DELAY)
    parser.add_argument("--output-audio-format", default=DEFAULT_OUTPUT_AUDIO_FORMAT, choices=['wav', 'mp3', 'ogg', 'flac'])
    parser.add_argument("--log-level", default="INFO", choices=["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"])
    parser.add_argument("--repair-mode", action="store_true", help="Enable repair mode to process only missing chunks based on existing temp files and metadata.")
    
    args = parser.parse_args() # These are the command-line arguments for the current run

    logging.getLogger().setLevel(args.log_level.upper())
    set_library_log_levels(args.log_level)
    logging.info(f"Book to Audio Script Version: {SCRIPT_VERSION}")
    try:
        gg_version = importlib.metadata.version("google-genai")
        logging.info(f"Using `google-genai` SDK version: {gg_version}")
    except: logging.warning("Could not determine `google-genai` SDK version.")

    dotenv_path = args.tool_root_dir / ".env"
    if dotenv_path.is_file(): load_dotenv(dotenv_path=dotenv_path); logging.info(f"Loaded .env file from: {dotenv_path}")
    else: logging.info(f".env file not found at {dotenv_path}.")

    # --- TEMPORARY DEBUG ---
    key_from_args_cli = args.api_key # Renamed to avoid confusion with actual arg name
    key_from_env_os = os.getenv("GOOGLE_API_KEY")
    logging.debug(f"TEMP DEBUG: Value from command-line --api-key (args.api_key): '{key_from_args_cli}'")
    logging.debug(f"TEMP DEBUG: Value from os.getenv('GOOGLE_API_KEY') after load_dotenv: '{key_from_env_os}'") # THIS IS KEY
    # --- END TEMPORARY DEBUG ---

    api_key_to_use = args.api_key or os.getenv("GOOGLE_API_KEY")
    if not api_key_to_use:
        logging.critical("API Key not found.")
        return

    tool_config = load_project_config(args.tool_root_dir)
    content_project_dir_str = tool_config.get("content_project_dir")
    if not content_project_dir_str: logging.critical(f"content_project_dir not in config.toml"); return
    content_project_dir = Path(content_project_dir_str).resolve()

    input_text_file = content_project_dir / "generated_tts_input" / args.input_filename
    if not input_text_file.exists(): logging.critical(f"Input text file not found: {input_text_file}"); return
    
    output_audio_dir = content_project_dir / "audio"
    output_audio_dir.mkdir(parents=True, exist_ok=True)
    output_audio_file_path = output_audio_dir / f"{Path(args.input_filename).stem}.{args.output_audio_format}"
    
    temp_chunk_base_dir = output_audio_dir / TEMP_DIR_NAME / Path(args.input_filename).stem
    metadata_file = temp_chunk_base_dir / METADATA_FILENAME

    # --- Determine Effective Arguments (handle repair mode and metadata) ---
    effective_args = argparse.Namespace(**vars(args)) # Start with current args

    if args.repair_mode:
        logging.info(f"Repair mode activated. Attempting to load metadata from: {metadata_file}")
        if not metadata_file.exists():
            logging.error(f"Repair mode: Metadata file {metadata_file} not found. Cannot proceed with repair. Run in normal mode first.")
            return
        try:
            with open(metadata_file, 'r') as mf:
                stored_metadata = json.load(mf)
            logging.info(f"Successfully loaded metadata: {stored_metadata}")

            # Critical parameters for chunking and content - MUST match or use stored.
            # For repair, we MUST use the stored chunk_max_chars and tts_prompt_prefix.
            if 'chunk_max_chars' not in stored_metadata or 'tts_prompt_prefix' not in stored_metadata:
                logging.error("Repair mode: Metadata is missing 'chunk_max_chars' or 'tts_prompt_prefix'. Cannot ensure deterministic chunking.")
                return

            logging.info(f"Repair mode: Overriding chunk_max_chars with stored value: {stored_metadata['chunk_max_chars']}")
            effective_args.chunk_max_chars = stored_metadata['chunk_max_chars']
            
            logging.info(f"Repair mode: Overriding tts_prompt_prefix with stored value.") # Don't log the prefix itself if long
            effective_args.tts_prompt_prefix = stored_metadata['tts_prompt_prefix']

            # Validate or override other parameters based on preference
            # For now, let's use stored values for model and voice for consistency in repair
            if 'model_name' in stored_metadata:
                if args.model_name != stored_metadata['model_name']:
                    logging.warning(f"Repair mode: Current model_name '{args.model_name}' differs from stored '{stored_metadata['model_name']}'. Using stored value for repair.")
                effective_args.model_name = stored_metadata['model_name']
            
            if 'voice_name' in stored_metadata:
                if args.voice_name != stored_metadata['voice_name']:
                     logging.warning(f"Repair mode: Current voice_name '{args.voice_name}' differs from stored '{stored_metadata['voice_name']}'. Using stored value for repair.")
                effective_args.voice_name = stored_metadata['voice_name']
            
            # Other args like retries, delay, concurrency can use current command-line values for the repair *attempt*
            logging.info(f"Repair mode: Using current run's max_api_retries ({args.max_api_retries}), retry_delay ({args.retry_delay}), concurrent_requests ({args.concurrent_requests}).")

        except Exception as e:
            logging.error(f"Repair mode: Error loading or processing metadata file {metadata_file}: {e}", exc_info=True)
            return
    else: # Normal mode
        # Ensure the temp_chunk_base_dir exists for metadata saving BEFORE chunking
        temp_chunk_base_dir.mkdir(parents=True, exist_ok=True)
        # In normal mode, effective_args are just args. Metadata will be saved using these.


    logging.info(f"Effective parameters for this run: Model={effective_args.model_name}, Voice={effective_args.voice_name}, ChunkMaxChars={effective_args.chunk_max_chars}")

    logging.info(f"Reading text from: {input_text_file}")
    try:
        with open(input_text_file, "r", encoding="utf-8") as f: full_text = f.read()
    except Exception as e: logging.critical(f"Error reading input file: {e}"); return
    if not full_text.strip(): logging.critical(f"Input file {input_text_file} is empty."); return

    logging.info(f"Original text length: {len(full_text)} characters.")
    # Chunking MUST use the effective_args.chunk_max_chars for consistency
    text_chunks = chunk_text(full_text, effective_args.chunk_max_chars)
    logging.info(f"Text divided into {len(text_chunks)} chunks for TTS processing (using chunk_max_chars: {effective_args.chunk_max_chars}).")
    if not text_chunks: logging.critical("Text chunking resulted in zero chunks."); return

    # Client uses current API key
    try:
        client = google_genai.Client(api_key=api_key_to_use)
        logging.info(f"Google GenAI client configured. Will use model: {effective_args.model_name} for TTS requests based on effective args.")
    except Exception as e:
        logging.critical(f"Failed to configure Google GenAI client: {e}", exc_info=True)
        return

    start_time = time.time()
    # Pass both original args (for things like repair_mode flag) and effective_args (for processing)
    await process_book_to_audio_async(text_chunks, client, output_audio_file_path, args, effective_args)
    end_time = time.time()
    logging.info(f"Total processing time: {end_time - start_time:.2f} seconds.")

if __name__ == "__main__":
    asyncio.run(main_async())