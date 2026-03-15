# -----------------------------------------------------------------------------
# book_to_audio.py
# Script Version: 1.5.0 (Multi-voice, Robust Merging, Quota Handling)
# -----------------------------------------------------------------------------

import asyncio
import argparse
from pathlib import Path
import os
import logging
import wave
import time
import importlib.metadata
import json
from typing import Any
import sys # <-- NEW: For sys.exit()

SCRIPT_VERSION = "1.5.0"

# --- NEW: Custom exception for handling daily quota limits ---
class QuotaExhaustedError(Exception):
    """Custom exception for when the daily API quota is hit."""
    pass
# --- END NEW ---

# --- Logging Setup (unchanged) ---
logging.basicConfig(format='%(asctime)s - %(levelname)s - %(name)s - %(message)s')

def set_library_log_levels(script_log_level_str: str):
    level = logging.DEBUG if script_log_level_str.upper() == "DEBUG" else logging.WARNING
    libraries_to_set = ["httpx", "google.genai._base_client", "google.api_core.retry", "google.auth.transport.requests", "google.cloud.texttospeech", "vertexai"]
    for lib_name in libraries_to_set:
        logging.getLogger(lib_name).setLevel(level)

# --- Library Imports (unchanged) ---
try:
    from google import genai as google_genai
    from google.genai import types as genai_types
except ImportError:
    logging.warning("Google GenAI (Gemini) library not found. Gemini API auth will not be available.")
    google_genai = None; genai_types = None
try:
    from google.cloud import texttospeech_v1 as texttospeech
    import google.auth
except ImportError:
    logging.warning("Google Cloud Text-to-Speech library not found. Old Vertex AI TTS will not be available.")
    texttospeech = None; google = None
try:
    import vertexai
    from vertexai.generative_models import GenerativeModel
except ImportError:
    logging.warning("Vertex AI SDK (`google-cloud-aiplatform`) not found. Gemini via Vertex AI auth will not be available.")
    vertexai = None
    GenerativeModel = None
from google.api_core import exceptions as api_core_exceptions
from dotenv import load_dotenv
from pydub import AudioSegment
import tomllib

# --- Configuration (Defaults are unchanged) ---
DEFAULT_TTS_SERVICE = "gemini"
DEFAULT_GEMINI_MODEL_NAME = "models/gemini-2.5-pro-preview-tts"
DEFAULT_GEMINI_VOICE_NAME = "Schedar"
DEFAULT_GEMINI_TTS_PROMPT_PREFIX = "You have a Mexican Spanish accent. Narrate the following text in a clear, even voice, suitable for an audiobook. You are telling a story. Be engaging:"
DEFAULT_VERTEX_VOICE_NAME = "en-US-Standard-C"
DEFAULT_VERTEX_LANGUAGE_CODE = "en-US"
DEFAULT_CHUNK_MAX_CHARS = 750
DEFAULT_CONCURRENT_REQUESTS = 2
DEFAULT_API_RETRIES = 10
DEFAULT_RETRY_DELAY = 15
DEFAULT_OUTPUT_AUDIO_FORMAT = "wav"
TEMP_DIR_NAME = "_tts_temp_chunks"
METADATA_FILENAME = "_metadata.json"
REQUEST_TIMEOUT_SECONDS = 500
PCM_CHANNELS = 1
PCM_FRAME_RATE = 24000
PCM_SAMPLE_WIDTH = 2

# --- Helper Functions (unchanged) ---
def load_project_config(tool_root_dir: Path) -> dict:
    config_file_path = tool_root_dir / "config.toml"
    if not config_file_path.exists(): logging.error(f"Tool config.toml not found at {config_file_path}"); return {}
    try:
        with open(config_file_path, "rb") as f: data = tomllib.load(f)
        return data
    except Exception as e:
        logging.error(f"Error loading tool config.toml: {e}"); return {}

def save_raw_pcm_to_wav(filename: Path, pcm_data: bytes, channels: int, rate: int, sample_width: int):
    with wave.open(str(filename), "wb") as wf:
        wf.setnchannels(channels); wf.setsampwidth(sample_width); wf.setframerate(rate); wf.writeframes(pcm_data)
    logging.debug(f"Raw PCM data saved to WAV: {filename}")

def chunk_text(full_text: str, max_chars: int) -> list[str]:
    chunks = []; current_pos = 0; text_len = len(full_text)
    while current_pos < text_len:
        end_pos = min(current_pos + max_chars, text_len); chunk = full_text[current_pos:end_pos]
        if end_pos < text_len:
            para_break = chunk.rfind('\n\n')
            if para_break != -1 and para_break > max_chars // 3 : chunk = chunk[:para_break + 2]; end_pos = current_pos + len(chunk)
            else:
                sentence_break = -1
                for sb_marker in ['. ', '! ', '? ', '." ', '!" ', '?" ','.\n', '!\n', '?\n']:
                    s_idx = chunk.rfind(sb_marker)
                    if s_idx != -1: potential_break = s_idx + len(sb_marker);
                    if potential_break > sentence_break: sentence_break = potential_break
                if sentence_break != -1 and sentence_break > max_chars // 3: chunk = chunk[:sentence_break]; end_pos = current_pos + len(chunk)
        final_chunk = chunk.strip()
        if final_chunk: chunks.append(final_chunk)
        current_pos = end_pos
    return chunks

# --- START: MODIFIED FUNCTION (Smart Quota Handling & Multiple Voices) ---
async def generate_audio_chunk_async(
    client: Any, 
    text_chunk: str,
    chunk_index: int,
    effective_args: argparse.Namespace,
    semaphore: asyncio.Semaphore,
    temp_chunk_dir: Path,
    voice_for_this_chunk: str # <-- NEW: Pass the specific voice for this chunk
) -> Path | None:
    try:
        async with semaphore:
            logging.info(f"[{effective_args.tts_service.upper()}] Requesting TTS for chunk {chunk_index + 1} with voice '{voice_for_this_chunk}'...")
            last_exception = None; audio_data_bytes = None
            if not text_chunk.strip():
                silence = AudioSegment.silent(duration=100, frame_rate=PCM_FRAME_RATE)
                temp_file_path = temp_chunk_dir / f"temp_chunk_{chunk_index:04d}_silence.wav"
                silence.export(str(temp_file_path), format="wav", parameters=["-ac", str(PCM_CHANNELS), "-ar", str(PCM_FRAME_RATE)])
                return temp_file_path

            for attempt in range(effective_args.max_api_retries):
                try:
                    api_call_description = f"Chunk {chunk_index + 1} (Attempt {attempt + 1}/{effective_args.max_api_retries})"
                    if effective_args.tts_service == "gemini":
                        if not google_genai or not genai_types: raise RuntimeError("Gemini API library not available.")
                        full_prompt = f"{effective_args.tts_prompt_prefix}\n\n{text_chunk}"
                        api_config = genai_types.GenerateContentConfig(
                            response_modalities=["audio"],
                            speech_config=genai_types.SpeechConfig(
                                voice_config=genai_types.VoiceConfig(
                                    prebuilt_voice_config=genai_types.PrebuiltVoiceConfig(
                                        voice_name=voice_for_this_chunk # <-- USE THE SPECIFIC VOICE
                                    )
                                )
                            ),
                        )
                        api_response = await asyncio.wait_for(client.aio.models.generate_content(
                                model=effective_args.model_name, contents=[full_prompt], config=api_config
                            ), timeout=REQUEST_TIMEOUT_SECONDS)

                        if (api_response.candidates and api_response.candidates[0].content and 
                            api_response.candidates[0].content.parts and 
                            api_response.candidates[0].content.parts[0].inline_data.data):
                            audio_data_bytes = api_response.candidates[0].content.parts[0].inline_data.data
                        else:
                            finish_reason = "N/A";
                            if api_response.candidates: finish_reason = str(api_response.candidates[0].finish_reason)
                            error_message = f"No audio data in Gemini response (FinishReason: {finish_reason}). This is often due to safety filters."
                            logging.error(f"{api_call_description} [GEMINI] - {error_message}")
                            logging.error(f"{api_call_description} [GEMINI] - Failing text (text_chunk):\n-------\n{text_chunk}\n-------")
                            last_exception = RuntimeError(error_message)
                    
                    elif effective_args.tts_service == "vertex":
                        # This section is unchanged but now uses `voice_for_this_chunk` for consistency
                        if not texttospeech: raise RuntimeError("Vertex AI Text-to-Speech library not available.")
                        synthesis_input = texttospeech.SynthesisInput(text=text_chunk)
                        voice_params = texttospeech.VoiceSelectionParams(language_code=effective_args.language_code, name=voice_for_this_chunk)
                        audio_config = texttospeech.AudioConfig(audio_encoding=texttospeech.AudioEncoding.LINEAR16, sample_rate_hertz=PCM_FRAME_RATE)
                        response = await client.synthesize_speech(request={"input": synthesis_input, "voice": voice_params, "audio_config": audio_config}, timeout=REQUEST_TIMEOUT_SECONDS)
                        if response.audio_content: audio_data_bytes = response.audio_content
                        else: last_exception = RuntimeError("No audio data in Vertex AI API response.")
                    else: raise ValueError(f"Unsupported TTS service: {effective_args.tts_service}")
                    
                    if audio_data_bytes:
                        temp_file_path = temp_chunk_dir / f"temp_chunk_{chunk_index:04d}.wav"
                        save_raw_pcm_to_wav(temp_file_path, audio_data_bytes, PCM_CHANNELS, PCM_FRAME_RATE, PCM_SAMPLE_WIDTH)
                        logging.info(f"[{effective_args.tts_service.upper()}] Chunk {chunk_index + 1} successfully converted and saved.")
                        return temp_file_path
                except Exception as e:
                    # --- NEW: Smart Quota Handling ---
                    error_str = str(e).lower()
                    if "resource_exhausted" in error_str and "quota" in error_str:
                        logging.warning("Daily API quota has been reached. Halting all further API calls.")
                        raise QuotaExhaustedError(str(e)) # Propagate special error
                    # --- END NEW ---
                    logging.error(f"{api_call_description} [{effective_args.tts_service.upper()}] - Error: {e}", exc_info=False); last_exception = e
                
                if attempt + 1 < effective_args.max_api_retries:
                    delay = effective_args.retry_delay; error_str = str(last_exception).lower() if last_exception else ""
                    if any(sub in error_str for sub in ["429", "rate limit", "unavailable", "s503"]): delay = effective_args.retry_delay * (2 ** attempt)
                    logging.info(f"Retrying chunk {chunk_index + 1} after error. Waiting {delay} seconds...")
                    await asyncio.sleep(delay)
            
            logging.error(f"[{effective_args.tts_service.upper()}] Chunk {chunk_index + 1} failed after {effective_args.max_api_retries} retries. Last error: {last_exception}")
            return None
    finally:
        delay = effective_args.delay_between_chunks
        if delay > 0: logging.debug(f"Waiting for {delay} seconds before next chunk..."); await asyncio.sleep(delay)
# --- END MODIFIED FUNCTION ---

# --- START: MODIFIED FUNCTION (Voice Rotation & Robust Merging) ---
async def process_book_to_audio_async(
    text_chunks_from_file: list[str],
    client: Any,
    output_file_path: Path,
    args: argparse.Namespace,
    effective_args: argparse.Namespace
):
    semaphore = asyncio.Semaphore(effective_args.concurrent_requests)
    if args.chunks_dir:
        temp_chunk_dir = args.chunks_dir.resolve()
    else:
        temp_chunk_dir = output_file_path.parent / TEMP_DIR_NAME / output_file_path.stem
    temp_chunk_dir.mkdir(parents=True, exist_ok=True)
    metadata_file_path = temp_chunk_dir / METADATA_FILENAME
    
    logging.info(f"Temporary audio and text chunks are stored in: {temp_chunk_dir}")
    total_expected_chunks_count = len(text_chunks_from_file)
    
    logging.info("Saving source text for each chunk to the temporary directory...")
    for i, text_content in enumerate(text_chunks_from_file):
        text_file_path = temp_chunk_dir / f"temp_chunk_{i:04d}.txt"
        try:
            with open(text_file_path, 'w', encoding='utf-8') as f: f.write(text_content)
        except IOError as e:
            logging.warning(f"Could not write source text file for chunk {i+1}: {e}")
    logging.info("All source text files saved.")

    # --- NEW: Voice Rotation Logic ---
    voices = list(effective_args.voice_name) # Ensure it's a mutable list
    if total_expected_chunks_count % 2 == 0 and len(voices) > 1:
        logging.info(f"Even chunk count ({total_expected_chunks_count}). Rotating voice list for variety.")
        voices = voices[-1:] + voices[:-1] # Moves the last voice to the front
        logging.info(f"New voice order: {', '.join(voices)}")
    # --- END NEW ---

    tasks_to_generate = []
    if args.repair_mode or args.chunks_dir:
        logging.info("--- GAP-DETECTION MODE ---")
        for i, text_content in enumerate(text_chunks_from_file):
            if not text_content.strip(): continue
            expected_audio_file = temp_chunk_dir / f"temp_chunk_{i:04d}.wav"; expected_silence_file = temp_chunk_dir / f"temp_chunk_{i:04d}_silence.wav"
            is_valid_existing = False
            if expected_audio_file.exists() and expected_audio_file.stat().st_size > 44:
                try: AudioSegment.from_file(expected_audio_file); is_valid_existing = True; logging.info(f"Repair mode: Chunk {i+1} (audio) found valid.")
                except Exception: logging.warning(f"Repair mode: Chunk {i+1} ({expected_audio_file.name}) exists but is corrupt. Will regenerate.")
            if not is_valid_existing and expected_silence_file.exists() and expected_silence_file.stat().st_size > 44:
                try: AudioSegment.from_file(expected_silence_file); is_valid_existing = True; logging.info(f"Repair mode: Chunk {i+1} (silence) found valid.")
                except Exception: logging.warning(f"Repair mode: Chunk {i+1} ({expected_silence_file.name}) exists but is corrupt. Will regenerate.")
            if not is_valid_existing:
                logging.info(f"Repair mode: Chunk {i+1} needs generation.")
                tasks_to_generate.append((i, text_content))
    else: # Normal mode
        logging.info(f"--- NORMAL MODE ---")
        metadata_to_save = { "script_version": SCRIPT_VERSION, "input_filename": effective_args.input_filename, "tts_service": effective_args.tts_service, "chunk_max_chars": effective_args.chunk_max_chars, "total_expected_chunks": total_expected_chunks_count }
        if effective_args.tts_service == "gemini": metadata_to_save.update({"model_name": effective_args.model_name, "voice_name": effective_args.voice_name, "tts_prompt_prefix": effective_args.tts_prompt_prefix})
        elif effective_args.tts_service == "vertex": metadata_to_save.update({"voice_name": effective_args.voice_name, "language_code": effective_args.language_code})
        with open(metadata_file_path, 'w') as mf: json.dump(metadata_to_save, mf, indent=4)
        for i, text_content in enumerate(text_chunks_from_file):
            tasks_to_generate.append((i, text_content))

    if tasks_to_generate:
        logging.info(f"Preparing {len(tasks_to_generate)} chunks for API calls.")
        generation_tasks_for_asyncio = []
        for original_idx, text_content in tasks_to_generate:
            # --- NEW: Assign voice for this chunk ---
            voice_for_this_chunk = voices[original_idx % len(voices)]
            # --- END NEW ---
            generation_tasks_for_asyncio.append(
                generate_audio_chunk_async(client, text_content, original_idx, effective_args, semaphore, temp_chunk_dir, voice_for_this_chunk)
            )
        
        if generation_tasks_for_asyncio:
            results = await asyncio.gather(*generation_tasks_for_asyncio, return_exceptions=True)
            # --- NEW: Check for quota error after batch completes ---
            for res in results:
                if isinstance(res, QuotaExhaustedError):
                    raise res # Propagate the error up to main_async
            # --- END NEW ---
    else:
        logging.info("No chunks required TTS generation in this run.")

    if args.no_concat:
        logging.info("--no-concat flag set. Skipping final audio concatenation.")
        logging.info(f"Audio chunks are in: {temp_chunk_dir}")
        return

    # --- NEW: Robust Merging Pre-Check ---
    logging.info("Verifying all audio chunks are present before concatenation...")
    all_final_chunk_paths = []
    missing_chunks = []
    for i in range(total_expected_chunks_count):
        is_original_chunk_empty = not text_chunks_from_file[i].strip()
        expected_audio_file = temp_chunk_dir / f"temp_chunk_{i:04d}.wav"
        expected_silence_file = temp_chunk_dir / f"temp_chunk_{i:04d}_silence.wav"
        chosen_file_for_concat = None

        if is_original_chunk_empty:
            if expected_silence_file.exists() and expected_silence_file.stat().st_size > 44:
                chosen_file_for_concat = expected_silence_file
        else:
            if expected_audio_file.exists() and expected_audio_file.stat().st_size > 44:
                try: AudioSegment.from_file(expected_audio_file); chosen_file_for_concat = expected_audio_file
                except Exception as e: logging.warning(f"Excluding corrupt audio file for chunk {i+1}: {e}")
        
        if chosen_file_for_concat:
            all_final_chunk_paths.append(chosen_file_for_concat)
        elif not is_original_chunk_empty:
            missing_chunks.append(i + 1)
    
    if missing_chunks:
        logging.error(f"Cannot create final audio file. The following {len(missing_chunks)} audio chunk(s) are missing or corrupt:")
        missing_str = ", ".join(map(str, missing_chunks))
        logging.error(f"  -> Missing Chunks: {missing_str}")
        logging.error("Please re-run the script in --repair-mode to generate the missing files.")
        return # Exit the function, skipping concatenation
    # --- END NEW ---

    if not all_final_chunk_paths:
        logging.error("No valid audio chunk files found for concatenation. Final audio not created.")
        return

    def get_chunk_index_from_path(p: Path) -> int:
        try: return int(p.stem.split('_')[-1].replace("_silence", ""));
        except: return -1
    sorted_chunk_paths = sorted(all_final_chunk_paths, key=get_chunk_index_from_path)
    logging.info(f"Concatenating {len(sorted_chunk_paths)} valid audio chunks...")
    combined_audio = AudioSegment.empty()
    for chunk_path in sorted_chunk_paths:
        try: combined_audio += AudioSegment.from_file(chunk_path)
        except Exception as e: logging.error(f"Error loading audio chunk {chunk_path}: {e}. Skipping.")
    if len(combined_audio) == 0: logging.error("Combined audio is empty. Final file not created."); return
    try:
        output_file_path.parent.mkdir(parents=True, exist_ok=True)
        combined_audio.export(str(output_file_path), format=effective_args.output_audio_format)
        logging.info(f"Successfully created final audiobook: {output_file_path}")
    except Exception as e:
        logging.error(f"Error exporting final audio: {e}")

    logging.info(f"Cleanup of temporary files has been disabled. Audio and text chunks are preserved in '{temp_chunk_dir}'.")
# --- END MODIFIED FUNCTION ---

# --- START: MODIFIED FUNCTION (argparse & Quota Handling) ---
async def main_async():
    parser = argparse.ArgumentParser(description="Convert a book to audio using Google TTS services.")
    parser.add_argument("--use-vertex-auth-for-gemini", action="store_true", help="Use Vertex AI authentication for Gemini models (production quotas).")
    parser.add_argument("--gcloud-project", help="Your Google Cloud Project ID (optional, but recommended for clarity).")
    parser.add_argument("--delay-between-chunks", type=int, default=0, help="Seconds to wait after processing each chunk to avoid rate limits.")
    parser.add_argument("--input-filename", required=False, default=None)
    parser.add_argument("--input-file", type=Path, default=None, help="Full path to the input text file (overrides --input-filename + config.toml resolution).")
    parser.add_argument("--output-dir", type=Path, default=None, help="Full path to the output audio directory (overrides config.toml resolution).")
    parser.add_argument("--tool-root-dir", type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument("--tts-service", default=DEFAULT_TTS_SERVICE, choices=["gemini", "vertex"])
    parser.add_argument("--api-key", help="Google API Key (for Gemini free tier).")
    parser.add_argument("--model-name", default=DEFAULT_GEMINI_MODEL_NAME)
    parser.add_argument("--tts-prompt-prefix", default=DEFAULT_GEMINI_TTS_PROMPT_PREFIX)
    parser.add_argument("--language-code", default=DEFAULT_VERTEX_LANGUAGE_CODE)
    # --- NEW: Accept multiple voice names ---
    parser.add_argument("--voice-name", nargs='+', help=f"One or more voice names for the selected service. If multiple, voices will be cycled.")
    # --- END NEW ---
    parser.add_argument("--chunk-max-chars", type=int, default=DEFAULT_CHUNK_MAX_CHARS)
    parser.add_argument("--concurrent-requests", type=int, default=DEFAULT_CONCURRENT_REQUESTS)
    parser.add_argument("--max-api-retries", type=int, default=DEFAULT_API_RETRIES)
    parser.add_argument("--retry-delay", type=int, default=DEFAULT_RETRY_DELAY)
    parser.add_argument("--output-audio-format", default=DEFAULT_OUTPUT_AUDIO_FORMAT)
    parser.add_argument("--log-level", default="INFO", choices=["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"])
    parser.add_argument("--repair-mode", action="store_true")
    parser.add_argument("--chunks-dir", type=Path, default=None, help="Directory for audio chunks. Enables gap-detection mode.")
    parser.add_argument("--no-concat", action="store_true", help="Skip final audio concatenation.")
    parser.add_argument("--concat-only", action="store_true", help="Skip TTS generation and only concatenate existing chunks.")
    args = parser.parse_args()

    if args.voice_name is None:
        if args.tts_service == "gemini": args.voice_name = [DEFAULT_GEMINI_VOICE_NAME]
        elif args.tts_service == "vertex": args.voice_name = [DEFAULT_VERTEX_VOICE_NAME]
    
    logging.getLogger().setLevel(args.log_level.upper()); set_library_log_levels(args.log_level)
    logging.info(f"Book to Audio Script Version: {SCRIPT_VERSION}")
    load_dotenv(dotenv_path=args.tool_root_dir / ".env")
    
    client = None
    if not args.concat_only:
        if args.tts_service == "gemini":
            if not google_genai: logging.critical("Gemini API library not installed."); return
            try:
                if args.use_vertex_auth_for_gemini:
                    if not google.auth: logging.critical("Google Auth library not found."); return
                    logging.info("Attempting to authenticate using Google Cloud Application Default Credentials (ADC)...")
                    credentials, discovered_project_id = google.auth.default()
                    project_id_to_use = args.gcloud_project or discovered_project_id
                    if not project_id_to_use: logging.critical("Could not discover Google Cloud Project ID."); return
                    logging.info(f"Initializing Gemini client for Vertex AI project '{project_id_to_use}'...")
                    client = google_genai.Client(project=project_id_to_use, credentials=credentials)
                    logging.info("Gemini client configured for Vertex AI successfully.")
                else:
                    api_key_to_use = args.api_key or os.getenv("GOOGLE_API_KEY")
                    if not api_key_to_use: logging.critical("Gemini API Key not found."); return
                    client = google_genai.Client(api_key=api_key_to_use)
                    logging.info("Gemini API (free tier) client configured.")
            except Exception as e:
                logging.critical(f"Failed to configure Gemini client. Error: {e}", exc_info=True); return

        elif args.tts_service == "vertex":
            if not texttospeech: logging.critical("Google Cloud Text-to-Speech library not installed."); return
            try: client = texttospeech.TextToSpeechAsyncClient()
            except Exception as e: logging.critical(f"Failed to configure Vertex AI client: {e}"); return
    
    # --- Path resolution: explicit paths take priority over config.toml ---
    if args.input_file and args.output_dir:
        # Rust app path mode: full paths provided directly
        input_text_file = args.input_file.resolve()
        output_audio_dir = args.output_dir.resolve()
        output_audio_file_path = output_audio_dir / f"{input_text_file.stem}.{args.output_audio_format}"
        # Populate input_filename for metadata (used in process_book_to_audio_async)
        if not args.input_filename:
            args.input_filename = input_text_file.name
    elif args.input_filename:
        # Legacy CLI mode: resolve from config.toml
        tool_config = load_project_config(args.tool_root_dir)
        content_project_dir_str = tool_config.get("content_project_dir"); content_project_dir = Path(content_project_dir_str).resolve()
        input_text_file = content_project_dir / "generated_tts_input" / args.input_filename
        output_audio_dir = content_project_dir / "audio"; output_audio_file_path = output_audio_dir / f"{Path(args.input_filename).stem}.{args.output_audio_format}"
    else:
        logging.critical("Either --input-file + --output-dir or --input-filename must be provided."); return

    # --- concat-only mode: skip TTS, just concatenate existing chunks ---
    if args.concat_only:
        if not args.chunks_dir:
            logging.critical("--concat-only requires --chunks-dir"); return
        temp_chunk_dir = args.chunks_dir.resolve()
        if not temp_chunk_dir.exists():
            logging.critical(f"Chunks directory not found: {temp_chunk_dir}"); return
        logging.info(f"Concat-only mode: concatenating chunks from {temp_chunk_dir}")

        chunk_files = []
        for f in sorted(temp_chunk_dir.iterdir()):
            name = f.name
            if name.endswith('.wav.bad'):
                continue
            if name.startswith('temp_chunk_') and name.endswith('.wav'):
                try:
                    AudioSegment.from_file(f)
                    chunk_files.append(f)
                except Exception as e:
                    logging.warning(f"Skipping corrupt chunk {name}: {e}")

        if not chunk_files:
            logging.error("No valid audio chunks found for concatenation."); return

        def get_idx(p: Path) -> int:
            try: return int(p.stem.replace("_silence", "").split('_')[-1])
            except: return -1

        sorted_chunks = sorted(chunk_files, key=get_idx)
        logging.info(f"Concatenating {len(sorted_chunks)} audio chunks...")
        combined_audio = AudioSegment.empty()
        for chunk_path in sorted_chunks:
            try: combined_audio += AudioSegment.from_file(chunk_path)
            except Exception as e: logging.error(f"Error loading chunk {chunk_path}: {e}. Skipping.")
        if len(combined_audio) == 0:
            logging.error("Combined audio is empty. Final file not created."); return

        output_audio_file_path.parent.mkdir(parents=True, exist_ok=True)
        combined_audio.export(str(output_audio_file_path), format=args.output_audio_format)
        logging.info(f"Successfully rebuilt audio: {output_audio_file_path}")
        return
    # --- end concat-only mode ---

    import re

    raw_text = input_text_file.read_text(encoding="utf-8")
    
    # 1. Replace any sequence of two or more newlines with a single newline.
    #    This collapses paragraph breaks but preserves intentional single line breaks.
    text_with_single_breaks = re.sub(r'\n{2,}', '\n', raw_text)
    
    # 2. Heuristic: For lines that do NOT end in punctuation, the single newline
    #    provides a good pause. For lines that DO end in punctuation, the newline
    #    can create an unnaturally long pause. We'll replace the newline with a space
    #    in those specific cases.
    final_text_parts = []
    for line in text_with_single_breaks.splitlines():
        stripped_line = line.strip()
        if stripped_line:
            # Check if the line ends with common sentence-ending punctuation.
            if stripped_line.endswith(('.', '!', '?', '"', '”')):
                final_text_parts.append(stripped_line)
            else:
                # If it's a heading or title, keep the line break for pacing.
                # We'll represent this with a unique placeholder for now.
                final_text_parts.append(stripped_line + "<PAUSE>")

    # Join everything with spaces, then replace the placeholder with a newline.
    # This ensures that even after joining, our intentional breaks are preserved.
    full_text = ' '.join(final_text_parts).replace("<PAUSE>", "\n")


    effective_args = argparse.Namespace(**vars(args))
    text_chunks = chunk_text(full_text, effective_args.chunk_max_chars)
    start_time = time.time()
    
    # --- NEW: Graceful exit for quota errors ---
    try:
        await process_book_to_audio_async(text_chunks, client, output_audio_file_path, args, effective_args)
    except QuotaExhaustedError:
        logging.info("Process stopped gracefully due to API quota exhaustion. You can resume tomorrow by re-running in --repair-mode.")
        sys.exit(0) # Exit with success code, as this is an expected stop condition.
    # --- END NEW ---
    
    logging.info(f"Total processing time: {time.time() - start_time:.2f} seconds.")
# --- END MODIFIED FUNCTION ---

if __name__ == "__main__":
    asyncio.run(main_async())