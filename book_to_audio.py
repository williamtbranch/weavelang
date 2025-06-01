# -----------------------------------------------------------------------------
# book_to_audio.py
# Script Version: 1.2.0 (Added Vertex AI TTS support, dual model)
# ...
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
from typing import Any # For client type hint

SCRIPT_VERSION = "1.2.0"

# --- Enhanced Debug Logging Setup ---
logging.basicConfig(format='%(asctime)s - %(levelname)s - %(name)s - %(message)s')

def set_library_log_levels(script_log_level_str: str):
    level = logging.DEBUG if script_log_level_str.upper() == "DEBUG" else logging.WARNING
    
    libraries_to_set = [
        "httpx",
        "google.generativeai._base_client", # Old SDK path
        "google.genai._base_client",      # New SDK path
        "google.genai.client",            # New SDK path
        "google.api_core.retry",
        "google.api_core.grpc_helpers",
        "google.auth.transport.requests",
        "google.cloud.texttospeech",      # Vertex AI TTS library
        "google.api_core.bidi"            # Often verbose with gRPC
    ]
    for lib_name in libraries_to_set:
        logging.getLogger(lib_name).setLevel(level)

# Attempt to import necessary libraries
try:
    from google import genai as google_genai
    from google.genai import types as genai_types
except ImportError:
    logging.warning("Google GenAI (Gemini) library not found. Gemini TTS will not be available.")
    google_genai = None
    genai_types = None

try:
    from google.cloud import texttospeech_v1 as texttospeech
    import google.auth # For Vertex auth errors
except ImportError:
    logging.warning("Google Cloud Text-to-Speech library not found. Vertex AI TTS will not be available.")
    texttospeech = None
    google = None # To prevent AttributeError if google.auth is accessed

from google.api_core import exceptions as api_core_exceptions
from dotenv import load_dotenv
from pydub import AudioSegment
import tomllib

# --- Configuration ---
DEFAULT_TTS_SERVICE = "gemini" # "gemini" or "vertex"

# Gemini Defaults
DEFAULT_GEMINI_MODEL_NAME = "models/gemini-1.5-flash-preview-tts" # Or your preferred Gemini TTS model
DEFAULT_GEMINI_VOICE_NAME = "Schedar"
DEFAULT_GEMINI_TTS_PROMPT_PREFIX = "You have a Mexican Spanish accent. Narrate the following text in a clear, even voice, suitable for an audiobook. You are telling a story. Be engaging:"

# Vertex AI Defaults
DEFAULT_VERTEX_VOICE_NAME = "en-US-Standard-C" # Example, choose from Vertex AI voice list
DEFAULT_VERTEX_LANGUAGE_CODE = "en-US"    # Example, e.g., "es-MX", "fr-FR"

DEFAULT_CHUNK_MAX_CHARS = 750
DEFAULT_CONCURRENT_REQUESTS = 2 
DEFAULT_API_RETRIES = 10
DEFAULT_RETRY_DELAY = 15 # seconds
DEFAULT_OUTPUT_AUDIO_FORMAT = "wav"
TEMP_DIR_NAME = "_tts_temp_chunks"
METADATA_FILENAME = "_metadata.json"
REQUEST_TIMEOUT_SECONDS = 150 # Timeout for the asyncio.wait_for() wrapper or individual API call

PCM_CHANNELS = 1
PCM_FRAME_RATE = 24000 # Supported by both Gemini TTS and Vertex AI (for LINEAR16)
PCM_SAMPLE_WIDTH = 2 # For 16-bit PCM

# --- Helper Functions ---
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
            if para_break != -1 and para_break > max_chars // 3 :
                chunk = chunk[:para_break + 2]
                end_pos = current_pos + len(chunk)
            else:
                sentence_break = -1
                for sb_marker in ['. ', '! ', '? ', '." ', '!" ', '?" ','.\n', '!\n', '?\n']:
                    s_idx = chunk.rfind(sb_marker)
                    if s_idx != -1:
                        potential_break = s_idx + len(sb_marker)
                        if potential_break > sentence_break: sentence_break = potential_break
                if sentence_break != -1 and sentence_break > max_chars // 3:
                    chunk = chunk[:sentence_break]
                    end_pos = current_pos + len(chunk)
        final_chunk = chunk.strip()
        if final_chunk: chunks.append(final_chunk)
        current_pos = end_pos
    return chunks

async def generate_audio_chunk_async(
    client: Any, # Can be Gemini or Vertex client
    text_chunk: str,
    chunk_index: int,
    effective_args: argparse.Namespace, # Contains resolved parameters for the chosen service
    semaphore: asyncio.Semaphore,
    temp_chunk_dir: Path
) -> Path | None:
    async with semaphore:
        logging.info(f"[{effective_args.tts_service.upper()}] Requesting TTS for chunk {chunk_index + 1} (Original Index)...")
        last_exception = None
        audio_data_bytes = None

        if not text_chunk.strip():
            logging.warning(f"[{effective_args.tts_service.upper()}] Chunk {chunk_index + 1} is empty. Creating 100ms silence.")
            silence = AudioSegment.silent(duration=100, frame_rate=PCM_FRAME_RATE)
            temp_file_path = temp_chunk_dir / f"temp_chunk_{chunk_index:04d}_silence.wav"
            silence.export(str(temp_file_path), format="wav", parameters=["-ac", str(PCM_CHANNELS), "-ar", str(PCM_FRAME_RATE)])
            return temp_file_path

        for attempt in range(effective_args.max_api_retries):
            try:
                api_call_description = f"Chunk {chunk_index + 1} (Attempt {attempt + 1}/{effective_args.max_api_retries})"
                if effective_args.tts_service == "gemini":
                    if not google_genai or not genai_types: # Should have been caught earlier
                        raise RuntimeError("Gemini library not available.")
                    
                    content_for_api = f"{effective_args.tts_prompt_prefix}\n\n{text_chunk}"
                    logging.debug(f"{api_call_description} [GEMINI]: Prompt+text length: {len(content_for_api)} chars. Model: {effective_args.model_name}, Voice: {effective_args.voice_name}")
                    
                    speech_config_dict = {"voice_config": {"prebuilt_voice_config": {"voice_name": effective_args.voice_name}}}
                    api_config = genai_types.GenerateContentConfig(
                        response_modalities=["AUDIO"],
                        speech_config=speech_config_dict
                    )
                    api_response = await asyncio.wait_for(
                        client.aio.models.generate_content(
                            model=effective_args.model_name, contents=content_for_api, config=api_config
                        ), timeout=REQUEST_TIMEOUT_SECONDS
                    )
                    if api_response.candidates and api_response.candidates[0].content and \
                       api_response.candidates[0].content.parts and \
                       api_response.candidates[0].content.parts[0].inline_data and \
                       api_response.candidates[0].content.parts[0].inline_data.data:
                        audio_data_bytes = api_response.candidates[0].content.parts[0].inline_data.data
                    else:
                        finish_reason_str = "N/A"
                        if api_response.candidates and api_response.candidates[0].finish_reason:
                            finish_reason_str = str(api_response.candidates[0].finish_reason)
                        error_message = f"No audio data in Gemini API response (200 OK but FinishReason: {finish_reason_str})."
                        logging.error(f"{api_call_description} [GEMINI] - {error_message}")
                        logging.error(f"{api_call_description} [GEMINI] - Failing text (text_chunk):\n-------\n{text_chunk}\n-------")
                        logging.debug(f"Full Gemini API Response for errored chunk {chunk_index + 1}: {api_response}")
                        last_exception = RuntimeError(error_message)
                
                elif effective_args.tts_service == "vertex":
                    if not texttospeech: # Should have been caught earlier
                        raise RuntimeError("Vertex AI Text-to-Speech library not available.")

                    logging.debug(f"{api_call_description} [VERTEX]: Text length: {len(text_chunk)} chars. Voice: {effective_args.voice_name}, Lang: {effective_args.language_code}")
                    
                    synthesis_input = texttospeech.SynthesisInput(text=text_chunk)
                    voice_params = texttospeech.VoiceSelectionParams(
                        language_code=effective_args.language_code,
                        name=effective_args.voice_name
                    )
                    audio_config = texttospeech.AudioConfig(
                        audio_encoding=texttospeech.AudioEncoding.LINEAR16,
                        sample_rate_hertz=PCM_FRAME_RATE,
                        # effects_profile_id=["telephony-class-application"] # Example for specific effects
                    )
                    
                    # Vertex client's synthesize_speech has its own timeout parameter.
                    # asyncio.wait_for could be an outer timeout if needed.
                    response = await client.synthesize_speech(
                        request={
                            "input": synthesis_input,
                            "voice": voice_params,
                            "audio_config": audio_config,
                        },
                        timeout=REQUEST_TIMEOUT_SECONDS # Timeout for the gRPC call itself
                    )
                    if response.audio_content:
                        audio_data_bytes = response.audio_content
                    else:
                        error_message = "No audio data in Vertex AI API response."
                        logging.error(f"{api_call_description} [VERTEX] - {error_message}")
                        logging.error(f"{api_call_description} [VERTEX] - Failing text (text_chunk):\n-------\n{text_chunk}\n-------")
                        logging.debug(f"Full Vertex API Response for errored chunk {chunk_index + 1}: {response}")
                        last_exception = RuntimeError(error_message)
                else:
                    raise ValueError(f"Unsupported TTS service in effective_args: {effective_args.tts_service}")

                if audio_data_bytes:
                    temp_file_path = temp_chunk_dir / f"temp_chunk_{chunk_index:04d}.wav"
                    save_raw_pcm_to_wav(temp_file_path, audio_data_bytes, PCM_CHANNELS, PCM_FRAME_RATE, PCM_SAMPLE_WIDTH)
                    logging.info(f"[{effective_args.tts_service.upper()}] Chunk {chunk_index + 1} successfully converted and saved to {temp_file_path}.")
                    return temp_file_path
                
                # If we are here, audio_data_bytes was None and last_exception should be set

            except asyncio.TimeoutError as e: # From asyncio.wait_for (primarily for Gemini)
                logging.error(f"{api_call_description} [{effective_args.tts_service.upper()}] - Application-level timeout via asyncio.wait_for after {REQUEST_TIMEOUT_SECONDS}s.")
                last_exception = e
            except api_core_exceptions.DeadlineExceeded as e: # From SDKs (gRPC timeout)
                logging.error(f"{api_call_description} [{effective_args.tts_service.upper()}] - SDK DeadlineExceeded: {e}", exc_info=True if effective_args.log_level.upper() == "DEBUG" else False)
                last_exception = e
            except api_core_exceptions.GoogleAPICallError as e: # General Google API error
                logging.error(f"{api_call_description} [{effective_args.tts_service.upper()}] - GoogleAPICallError: {e}", exc_info=True if effective_args.log_level.upper() == "DEBUG" else False)
                last_exception = e
            except TypeError as e: # SDK version or config mismatch
                sdk_version_str = "unknown"
                try: 
                    if effective_args.tts_service == "gemini" and google_genai: sdk_version_str = importlib.metadata.version("google-genai")
                    elif effective_args.tts_service == "vertex" and texttospeech: sdk_version_str = importlib.metadata.version("google-cloud-texttospeech")
                except: pass
                logging.error(f"{api_call_description} [{effective_args.tts_service.upper()}] - Critical TypeError: {e}. SDK version: {sdk_version_str}. Config/arguments might be incorrect.", exc_info=True)
                return None # Fatal for this chunk
            except AttributeError as e:
                logging.error(f"{api_call_description} [{effective_args.tts_service.upper()}] - AttributeError: {e}", exc_info=True)
                last_exception = e
            except Exception as e:
                logging.error(f"{api_call_description} [{effective_args.tts_service.upper()}] - General Error: {e}", exc_info=True)
                last_exception = e
            
            if attempt + 1 < effective_args.max_api_retries:
                delay = effective_args.retry_delay
                error_str = str(last_exception).lower() if last_exception else ""
                if any(sub in error_str for sub in ["429", "resource_exhausted", "rate limit", "unavailable", "503"]):
                    delay = effective_args.retry_delay * (2 ** attempt) # Exponential backoff
                    logging.info(f"Rate limit or temporary server issue likely hit for chunk {chunk_index + 1}. Retrying in {delay} seconds...")
                else:
                    logging.info(f"Retrying chunk {chunk_index + 1} (Attempt {attempt + 2}/{effective_args.max_api_retries}) after error. Waiting {delay} seconds...")
                await asyncio.sleep(delay)
        
        logging.error(f"[{effective_args.tts_service.upper()}] Chunk {chunk_index + 1} failed after {effective_args.max_api_retries} retries. Last error: {last_exception}")
        return None


async def process_book_to_audio_async(
    text_chunks_from_file: list[str],
    client: Any, # Gemini or Vertex client
    output_file_path: Path,
    args: argparse.Namespace, # Current run's command-line args
    effective_args: argparse.Namespace # Args to USE (from metadata in repair or current run)
):
    semaphore = asyncio.Semaphore(effective_args.concurrent_requests)
    temp_chunk_dir = output_file_path.parent / TEMP_DIR_NAME / output_file_path.stem
    temp_chunk_dir.mkdir(parents=True, exist_ok=True)
    metadata_file_path = temp_chunk_dir / METADATA_FILENAME
    
    logging.info(f"Temporary audio chunks for '{output_file_path.name}' will be stored in: {temp_chunk_dir}")
    total_expected_chunks_count = len(text_chunks_from_file)
    logging.info(f"Total expected chunks based on current/metadata settings: {total_expected_chunks_count}")

    tasks_to_generate = [] # List of (original_index, text_content)
    
    if args.repair_mode: # Repair mode logic now driven by `args.repair_mode`
        logging.info("--- REPAIR MODE ENABLED (using effective_args from metadata) ---")
        for i, text_content in enumerate(text_chunks_from_file):
            if not text_content.strip(): continue 

            expected_audio_file = temp_chunk_dir / f"temp_chunk_{i:04d}.wav"
            expected_silence_file = temp_chunk_dir / f"temp_chunk_{i:04d}_silence.wav"
            
            is_valid_existing = False
            if expected_audio_file.exists() and expected_audio_file.stat().st_size > 44:
                try: AudioSegment.from_file(expected_audio_file); is_valid_existing = True
                except: pass
                else: logging.info(f"Repair mode: Chunk {i+1} (audio) found valid: {expected_audio_file}")
            
            if not is_valid_existing and expected_silence_file.exists() and expected_silence_file.stat().st_size > 44:
                try: AudioSegment.from_file(expected_silence_file); is_valid_existing = True
                except: pass
                else: logging.info(f"Repair mode: Chunk {i+1} (silence) found valid: {expected_silence_file}")

            if not is_valid_existing:
                logging.info(f"Repair mode: Chunk {i+1} needs generation (using '{effective_args.tts_service}' service settings from metadata).")
                tasks_to_generate.append((i, text_content))
    else: # Normal mode
        logging.info(f"--- NORMAL MODE (using '{effective_args.tts_service}' service) ---")
        metadata_to_save = {
            "script_version": SCRIPT_VERSION,
            "input_filename": effective_args.input_filename,
            "tts_service": effective_args.tts_service,
            "chunk_max_chars": effective_args.chunk_max_chars,
            "total_expected_chunks": total_expected_chunks_count
        }
        if effective_args.tts_service == "gemini":
            metadata_to_save["model_name"] = effective_args.model_name
            metadata_to_save["voice_name"] = effective_args.voice_name
            metadata_to_save["tts_prompt_prefix"] = effective_args.tts_prompt_prefix
        elif effective_args.tts_service == "vertex":
            metadata_to_save["voice_name"] = effective_args.voice_name
            metadata_to_save["language_code"] = effective_args.language_code
        
        try:
            with open(metadata_file_path, 'w') as mf:
                json.dump(metadata_to_save, mf, indent=4)
            logging.info(f"Saved metadata to {metadata_file_path}")
        except IOError as e:
            logging.error(f"Could not save metadata file {metadata_file_path}: {e}")

        for i, text_content in enumerate(text_chunks_from_file):
            tasks_to_generate.append((i, text_content)) # generate_audio_chunk_async handles empty text

    # --- TTS Generation for required chunks ---
    generation_tasks_for_asyncio = []
    if tasks_to_generate:
        logging.info(f"Preparing {len(tasks_to_generate)} chunks for {effective_args.tts_service.upper()} API calls.")
        for original_idx, text_content_for_tts in tasks_to_generate:
            generation_tasks_for_asyncio.append(
                generate_audio_chunk_async(client, text_content_for_tts, original_idx, effective_args, semaphore, temp_chunk_dir)
            )
        
        if generation_tasks_for_asyncio:
            logging.info(f"Submitting {len(generation_tasks_for_asyncio)} tasks to asyncio.gather...")
            results_from_gather = await asyncio.gather(*generation_tasks_for_asyncio, return_exceptions=True)
            for i, res in enumerate(results_from_gather):
                if isinstance(res, Exception):
                    original_idx_for_this_task = tasks_to_generate[i][0]
                    logging.error(f"A generation task (for original chunk {original_idx_for_this_task+1}) resulted in an unhandled exception from asyncio.gather: {res}", exc_info=res if effective_args.log_level.upper() == "DEBUG" else False)
    else:
        logging.info("No chunks required TTS generation in this run.")

    # --- Concatenation Stage ---
    all_final_chunk_paths = []
    all_expected_chunks_accounted_for = True
    actual_non_empty_input_chunks_count = 0
    generated_content_chunks_count = 0 # Count of successfully generated or pre-existing valid chunks

    for i in range(total_expected_chunks_count):
        is_original_chunk_empty = not text_chunks_from_file[i].strip()
        
        expected_audio_file = temp_chunk_dir / f"temp_chunk_{i:04d}.wav"
        expected_silence_file = temp_chunk_dir / f"temp_chunk_{i:04d}_silence.wav"

        chosen_file_for_concat = None

        if is_original_chunk_empty:
            if expected_silence_file.exists() and expected_silence_file.stat().st_size > 44:
                try: AudioSegment.from_file(expected_silence_file); chosen_file_for_concat = expected_silence_file
                except: logging.warning(f"Silence file {expected_silence_file} for original empty chunk {i+1} is invalid.")
            else:
                logging.debug(f"Original chunk {i+1} was empty, and no corresponding silence file found. Assuming skipped as intended.")
        else: # Original chunk had content
            actual_non_empty_input_chunks_count += 1
            if expected_audio_file.exists() and expected_audio_file.stat().st_size > 44:
                try: AudioSegment.from_file(expected_audio_file); chosen_file_for_concat = expected_audio_file
                except Exception as e: logging.warning(f"Content file {expected_audio_file} for chunk {i+1} exists but is invalid: {e}")
        
        if chosen_file_for_concat:
            all_final_chunk_paths.append(chosen_file_for_concat)
            if not is_original_chunk_empty: # only count if it was meant to be content
                 generated_content_chunks_count +=1
        elif not is_original_chunk_empty : # Original chunk had content but no valid audio file found
            logging.error(f"Final check: Audio for original content chunk {i+1} (expected {expected_audio_file}) is missing or invalid before concatenation.")
            all_expected_chunks_accounted_for = False


    if not all_final_chunk_paths:
        logging.error("No valid audio chunk files found for concatenation. Final audio not created.")
        return

    def get_chunk_index_from_path(p: Path) -> int:
        try: return int(p.stem.split('_')[-1].replace("_silence", ""))
        except: return -1 # Should not happen with correct naming
    sorted_chunk_paths = sorted(all_final_chunk_paths, key=get_chunk_index_from_path)
    
    if generated_content_chunks_count != actual_non_empty_input_chunks_count:
         logging.warning(
            f"Mismatch: Concatenating {len(sorted_chunk_paths)} files. "
            f"Successfully processed/found {generated_content_chunks_count} audio files for "
            f"{actual_non_empty_input_chunks_count} non-empty input chunks. "
            f"Final audio may be incomplete if `all_expected_chunks_accounted_for` is False."
        )
    if not all_expected_chunks_accounted_for:
        logging.warning(f"Final audio WILL BE INCOMPLETE due to missing chunks for non-empty input text.")


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
            logging.error("Ensure FFmpeg/LibAV is installed and in your system's PATH for MP3 export.")

    if args.repair_mode or all_expected_chunks_accounted_for: # Use args.repair_mode for cleanup decision
        logging.info(f"Proceeding with cleanup of temporary files from {temp_chunk_dir}...")
        if temp_chunk_dir.exists() and temp_chunk_dir.is_dir():
            for item in temp_chunk_dir.iterdir():
                try:
                    if item.is_file(): os.remove(item)
                except OSError as e: logging.warning(f"Could not remove temp item {item}: {e}")
            try:
                if not any(temp_chunk_dir.iterdir()): os.rmdir(temp_chunk_dir)
                else: logging.info(f"Temp dir {temp_chunk_dir} not empty after file cleanup, not removed.")
            except OSError as e: logging.warning(f"Could not remove temp dir {temp_chunk_dir}: {e}")
    else:
        logging.warning(f"Not all chunks were successfully generated in this normal run. Temporary files in {temp_chunk_dir} (including metadata) will be kept for a potential --repair-mode run.")


async def main_async():
    parser = argparse.ArgumentParser(description="Convert a book text file to audio using Google GenAI TTS or Vertex AI TTS.")
    parser.add_argument("--input-filename", required=True)
    parser.add_argument("--tool-root-dir", type=Path, default=Path(__file__).resolve().parent)
    
    # Service selection
    parser.add_argument("--tts-service", default=DEFAULT_TTS_SERVICE, choices=["gemini", "vertex"],
                        help=f"TTS service to use. Default: {DEFAULT_TTS_SERVICE}")
    
    # Gemini specific (conditionally used)
    parser.add_argument("--api-key", help="Google API Key (primarily for Gemini TTS).")
    parser.add_argument("--model-name", default=DEFAULT_GEMINI_MODEL_NAME,
                        help=f"Gemini TTS model name. Default: {DEFAULT_GEMINI_MODEL_NAME}")
    parser.add_argument("--tts-prompt-prefix", default=DEFAULT_GEMINI_TTS_PROMPT_PREFIX,
                        help="Prompt prefix for Gemini TTS.")

    # Vertex AI specific (conditionally used)
    parser.add_argument("--language-code", default=DEFAULT_VERTEX_LANGUAGE_CODE,
                        help=f"Language code for Vertex AI TTS (e.g., en-US, es-MX). Default: {DEFAULT_VERTEX_LANGUAGE_CODE}")

    # Common or service-specific (voice_name)
    parser.add_argument("--voice-name", 
                        help="Voice name for the selected TTS service. "
                             f"Gemini default: {DEFAULT_GEMINI_VOICE_NAME}, Vertex default: {DEFAULT_VERTEX_VOICE_NAME}")
    
    # General processing
    parser.add_argument("--chunk-max-chars", type=int, default=DEFAULT_CHUNK_MAX_CHARS)
    parser.add_argument("--concurrent-requests", type=int, default=DEFAULT_CONCURRENT_REQUESTS)
    parser.add_argument("--max-api-retries", type=int, default=DEFAULT_API_RETRIES)
    parser.add_argument("--retry-delay", type=int, default=DEFAULT_RETRY_DELAY)
    parser.add_argument("--output-audio-format", default=DEFAULT_OUTPUT_AUDIO_FORMAT, choices=['wav', 'mp3', 'ogg', 'flac'])
    parser.add_argument("--log-level", default="INFO", choices=["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"])
    parser.add_argument("--repair-mode", action="store_true", help="Enable repair mode.")
    
    args = parser.parse_args()

    # Set default voice name based on service if not provided
    if args.voice_name is None:
        if args.tts_service == "gemini":
            args.voice_name = DEFAULT_GEMINI_VOICE_NAME
        elif args.tts_service == "vertex":
            args.voice_name = DEFAULT_VERTEX_VOICE_NAME
    
    logging.getLogger().setLevel(args.log_level.upper())
    set_library_log_levels(args.log_level)
    logging.info(f"Book to Audio Script Version: {SCRIPT_VERSION}")

    # Log SDK versions
    if args.tts_service == "gemini" and google_genai:
        try: logging.info(f"Using `google-genai` SDK version: {importlib.metadata.version('google-genai')}")
        except: logging.warning("Could not determine `google-genai` SDK version.")
    elif args.tts_service == "vertex" and texttospeech:
        try: logging.info(f"Using `google-cloud-texttospeech` SDK version: {importlib.metadata.version('google-cloud-texttospeech')}")
        except: logging.warning("Could not determine `google-cloud-texttospeech` SDK version.")


    dotenv_path = args.tool_root_dir / ".env"
    if dotenv_path.is_file(): load_dotenv(dotenv_path=dotenv_path); logging.info(f"Loaded .env file from: {dotenv_path}")
    
    client = None
    if args.tts_service == "gemini":
        if not google_genai:
            logging.critical("Gemini TTS service selected, but 'google-genai' library is not installed. Please install it.")
            return
        api_key_to_use = args.api_key or os.getenv("GOOGLE_API_KEY")
        if not api_key_to_use:
            logging.critical("Gemini TTS: API Key not found (provide via --api-key or GOOGLE_API_KEY in .env/environment).")
            return
        try:
            client = google_genai.Client(api_key=api_key_to_use)
            logging.info(f"Gemini client configured. Effective Model: {args.model_name}, Voice: {args.voice_name}")
        except Exception as e:
            logging.critical(f"Failed to configure Gemini client: {e}", exc_info=True)
            return
    elif args.tts_service == "vertex":
        if not texttospeech:
            logging.critical("Vertex AI TTS service selected, but 'google-cloud-texttospeech' library is not installed. Please install it.")
            return
        try:
            client = texttospeech.TextToSpeechAsyncClient()
            # Test call or project check can be added here if needed
            logging.info("Vertex AI TextToSpeechAsyncClient configured. (Uses Application Default Credentials)")
            logging.info(f"Vertex AI Effective Voice: {args.voice_name}, Language: {args.language_code}")
            logging.info("Ensure you have run 'gcloud auth application-default login' and enabled the Cloud Text-to-Speech API in your GCP project.")
        except google.auth.exceptions.DefaultCredentialsError as e:
            logging.critical(f"Vertex AI: Google Cloud Default Credentials not found or invalid: {e}")
            logging.critical("Please run 'gcloud auth application-default login' or set up credentials correctly.")
            return
        except Exception as e:
            logging.critical(f"Failed to configure Vertex AI client: {e}", exc_info=True)
            return
    else: # Should be caught by argparse choices
        logging.critical(f"Invalid TTS service selected: {args.tts_service}")
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

    effective_args = argparse.Namespace(**vars(args)) # Start with current args

    if args.repair_mode:
        logging.info(f"Repair mode activated. Attempting to load metadata from: {metadata_file}")
        if not metadata_file.exists():
            logging.error(f"Repair mode: Metadata file {metadata_file} not found. Cannot proceed. Run in normal mode first."); return
        try:
            with open(metadata_file, 'r') as mf: stored_metadata = json.load(mf)
            logging.info(f"Successfully loaded metadata: {stored_metadata}")

            if 'tts_service' not in stored_metadata:
                logging.error("Repair mode: Metadata is missing 'tts_service'. Cannot proceed with repair."); return
            
            # Override tts_service from metadata for the repair operations
            effective_args.tts_service = stored_metadata['tts_service']
            logging.info(f"Repair mode: Using TTS service from metadata: '{effective_args.tts_service}'")

            # Critical parameters for chunking and content - MUST match or use stored.
            if 'chunk_max_chars' not in stored_metadata:
                logging.error("Repair mode: Metadata is missing 'chunk_max_chars'. Cannot ensure deterministic chunking."); return
            effective_args.chunk_max_chars = stored_metadata['chunk_max_chars']
            logging.info(f"Repair mode: Overriding chunk_max_chars with stored value: {effective_args.chunk_max_chars}")

            if effective_args.tts_service == "gemini":
                for param in ["model_name", "voice_name", "tts_prompt_prefix"]:
                    if param not in stored_metadata: logging.error(f"Repair (Gemini): Metadata missing '{param}'."); return
                    setattr(effective_args, param, stored_metadata[param])
                    logging.info(f"Repair (Gemini): Using stored '{param}'.")
            elif effective_args.tts_service == "vertex":
                for param in ["voice_name", "language_code"]:
                    if param not in stored_metadata: logging.error(f"Repair (Vertex): Metadata missing '{param}'."); return
                    setattr(effective_args, param, stored_metadata[param])
                    logging.info(f"Repair (Vertex): Using stored '{param}'.")
            
            # Re-initialize client if the service from metadata differs from initial CLI-selected one
            # This is important if user ran --repair-mode --tts-service vertex but metadata was gemini
            if args.tts_service != effective_args.tts_service:
                logging.warning(f"Repair mode: CLI specified '{args.tts_service}' but metadata indicates '{effective_args.tts_service}'. Re-initializing client for '{effective_args.tts_service}'.")
                # (Re-initialization logic as above)
                if effective_args.tts_service == "gemini":
                    # ... (Gemini client init)
                    api_key_to_use = args.api_key or os.getenv("GOOGLE_API_KEY") # Still use current key for repair attempt
                    if not api_key_to_use: logging.critical("Gemini TTS (repair): API Key not found."); return
                    client = google_genai.Client(api_key=api_key_to_use)

                elif effective_args.tts_service == "vertex":
                    # ... (Vertex client init)
                    client = texttospeech.TextToSpeechAsyncClient()
                logging.info(f"Client re-initialized for '{effective_args.tts_service}' based on metadata.")


        except Exception as e:
            logging.error(f"Repair mode: Error loading or processing metadata file {metadata_file}: {e}", exc_info=True)
            return
    else: # Normal mode
        temp_chunk_base_dir.mkdir(parents=True, exist_ok=True) # Ensure dir for metadata
        # effective_args are already set from args for normal mode.

    logging.info(f"Effective parameters for this run ({effective_args.tts_service.upper()}):")
    if effective_args.tts_service == "gemini":
        logging.info(f"  Model={effective_args.model_name}, Voice={effective_args.voice_name}, ChunkMaxChars={effective_args.chunk_max_chars}")
        # logging.info(f"  Prompt Prefix: {effective_args.tts_prompt_prefix[:100]}...") # Avoid logging long prompts
    elif effective_args.tts_service == "vertex":
        logging.info(f"  Voice={effective_args.voice_name}, Language={effective_args.language_code}, ChunkMaxChars={effective_args.chunk_max_chars}")


    logging.info(f"Reading text from: {input_text_file}")
    try:
        with open(input_text_file, "r", encoding="utf-8") as f: full_text = f.read()
    except Exception as e: logging.critical(f"Error reading input file: {e}"); return
    if not full_text.strip(): logging.critical(f"Input file {input_text_file} is empty."); return

    logging.info(f"Original text length: {len(full_text)} characters.")
    text_chunks = chunk_text(full_text, effective_args.chunk_max_chars) # Chunking uses effective_args
    logging.info(f"Text divided into {len(text_chunks)} chunks (using chunk_max_chars: {effective_args.chunk_max_chars}).")
    if not text_chunks: logging.critical("Text chunking resulted in zero chunks."); return

    start_time = time.time()
    await process_book_to_audio_async(text_chunks, client, output_audio_file_path, args, effective_args)
    end_time = time.time()
    logging.info(f"Total processing time: {end_time - start_time:.2f} seconds.")

if __name__ == "__main__":
    asyncio.run(main_async())