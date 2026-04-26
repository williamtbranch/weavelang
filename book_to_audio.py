# -----------------------------------------------------------------------------
# book_to_audio.py
# Script Version: 1.6.0 (Multi-voice, Multi-speaker Interleave, Robust Merging, Quota Handling)
# -----------------------------------------------------------------------------

import asyncio
import argparse
from pathlib import Path
import struct
import mimetypes
import os
import logging
import wave
import time
import importlib.metadata
import json
import re
import hashlib
from typing import Any
import sys # <-- NEW: For sys.exit()

SCRIPT_VERSION = "1.6.0"

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
except (ImportError, MemoryError):
    logging.warning("Vertex AI SDK (`google-cloud-aiplatform`) not available. Gemini via Vertex AI auth will not be available.")
    vertexai = None
    GenerativeModel = None
from google.api_core import exceptions as api_core_exceptions
from pydub import AudioSegment
import tomllib

# --- Configuration (Defaults are unchanged) ---
DEFAULT_TTS_SERVICE = "gemini"
DEFAULT_GEMINI_MODEL_NAME = "models/gemini-2.5-pro-preview-tts"
DEFAULT_GEMINI_VOICE_NAME = "Schedar"
#DEFAULT_GEMINI_TTS_PROMPT_PREFIX = "You have a Mexican Spanish accent. You are performing a story from classic public domain literature from Project Gutenberg. Read the text in a clear, engaging, and natural way, as if narrating an audiobook. Use appropriate intonation and pacing to bring the story to life. Do not add any commentary or information that is not in the text. Just read the text as it is written."
DEFAULT_GEMINI_TTS_PROMPT_PREFIX = "You are performing a story from classic public domain literature from Project Gutenberg. Be upbeat. Read the text in a clear, engaging, and natural conversational way, as if narrating an audiobook. You have a Mexican Spanish accent. Use smooth phrasing and light energy. Avoid slow word-by-word delivery. Do not add any commentary or information that is not in the text."
DEFAULT_VERTEX_VOICE_NAME = "en-US-Standard-C"
DEFAULT_VERTEX_LANGUAGE_CODE = "en-US"
DEFAULT_CHUNK_MAX_CHARS = 750
DEFAULT_CONCURRENT_REQUESTS = 2
DEFAULT_API_RETRIES = 10
DEFAULT_RETRY_DELAY = 15
DEFAULT_OUTPUT_AUDIO_FORMAT = "wav"
TEMP_DIR_NAME = "_tts_temp_chunks"
METADATA_FILENAME = "_metadata.json"
CHUNK_MAP_FILENAME = "_chunk_map.json"
SF_ALIGNMENT_MAP_FILENAME = "_sf_alignment_map.json"
SF_ALIGNMENT_MAP_VERSION = 1
SF_CHUNK_META_FILENAME = "_sf_chunk_meta.json"
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

def is_interleave_file(filename: str) -> bool:
    """Return True if the filename matches the ULi interleave pattern (e.g. name_ULi34.txt)."""
    return bool(re.search(r'ULi\d+', filename))


def split_paragraphs(text: str) -> list[str]:
    """Split text into cleaned paragraphs on double-newline boundaries.
    This is the canonical sentence boundary used for SF alignment."""
    raw = re.split(r'\n{2,}', text)
    return [' '.join(p.split()) for p in raw if p.strip()]


def compute_source_hash(paragraphs: list[str]) -> str:
    """Compute a stable hash over paragraph structure (not content).
    Detects sentence insertions/deletions without being sensitive to
    per-level word changes."""
    sig = "|".join(str(len(p)) for p in paragraphs)
    return "sha256:" + hashlib.sha256(sig.encode('utf-8')).hexdigest()


def rechunk_by_alignment_map(paragraphs: list[str], chunks_from_map: list[dict]) -> list[tuple[str, int, int]]:
    """Rebuild text chunks from saved sentence ranges.
    Ensures all levels use identical chunk boundaries regardless of character widths."""
    result = []
    for chunk_def in chunks_from_map:
        start = chunk_def['start_sentence']  # 1-based
        end = chunk_def['end_sentence']       # 1-based, inclusive
        chunk_paras = paragraphs[start - 1:end]
        result.append((' '.join(chunk_paras), start, end))
    return result


def chunk_interleave_text(full_text: str, max_chars: int) -> list[str]:
    """Chunk interleave text by speaker pairs, respecting max_chars.

    Each group of consecutive Speaker lines (Speaker 1 + Speaker 2) is kept
    together. Groups are accumulated into chunks until adding another would
    exceed max_chars.
    """
    # Parse into groups: each group is a block of consecutive "Speaker N: ..." lines
    lines = [line.strip() for line in full_text.splitlines() if line.strip()]
    groups: list[str] = []
    current_group_lines: list[str] = []
    for line in lines:
        if re.match(r'^Speaker \d+:', line) and current_group_lines:
            # If this Speaker 1 starts a new triplet, flush the previous group
            if line.startswith('Speaker 1:') and current_group_lines:
                groups.append('\n'.join(current_group_lines))
                current_group_lines = []
        current_group_lines.append(line)
    if current_group_lines:
        groups.append('\n'.join(current_group_lines))

    # Now pack groups into chunks respecting max_chars
    chunks: list[str] = []
    current_chunk_parts: list[str] = []
    current_len = 0
    for group in groups:
        group_len = len(group)
        # If adding this group would exceed max_chars, flush current chunk
        if current_chunk_parts and current_len + group_len + 2 > max_chars:
            chunks.append('\n\n'.join(current_chunk_parts))
            current_chunk_parts = []
            current_len = 0
        current_chunk_parts.append(group)
        current_len += group_len + 2  # +2 for the \n\n separator
    if current_chunk_parts:
        chunks.append('\n\n'.join(current_chunk_parts))
    return chunks


def parse_audio_mime_type(mime_type: str) -> dict[str, int]:
    """Parse bits_per_sample and rate from an audio MIME type (e.g. 'audio/L16;rate=24000')."""
    bits_per_sample = 16
    rate = 24000
    parts = mime_type.split(";")
    for param in parts:
        param = param.strip()
        if param.lower().startswith("rate="):
            try:
                rate = int(param.split("=", 1)[1])
            except (ValueError, IndexError):
                pass
        elif param.startswith("audio/L"):
            try:
                bits_per_sample = int(param.split("L", 1)[1])
            except (ValueError, IndexError):
                pass
    return {"bits_per_sample": bits_per_sample, "rate": rate}


def convert_to_wav(audio_data: bytes, mime_type: str) -> bytes:
    """Wrap raw PCM audio data in a WAV header based on MIME type parameters."""
    parameters = parse_audio_mime_type(mime_type)
    bits_per_sample = parameters["bits_per_sample"]
    sample_rate = parameters["rate"]
    num_channels = 1
    data_size = len(audio_data)
    bytes_per_sample = bits_per_sample // 8
    block_align = num_channels * bytes_per_sample
    byte_rate = sample_rate * block_align
    chunk_size = 36 + data_size
    header = struct.pack(
        "<4sI4s4sIHHIIHH4sI",
        b"RIFF", chunk_size, b"WAVE", b"fmt ", 16, 1,
        num_channels, sample_rate, byte_rate, block_align, bits_per_sample,
        b"data", data_size,
    )
    return header + audio_data


def chunk_text(full_text: str, max_chars: int) -> list[tuple[str, int, int]]:
    """Split text into chunks, using double-newline paragraph boundaries.

    The woven text files use double newlines to separate sentences/paragraphs.
    We split on those boundaries first, clean each paragraph (collapse internal
    newlines to spaces), then pack whole paragraphs into chunks up to max_chars.
    A single paragraph that exceeds max_chars is emitted as its own chunk
    (the TTS API can handle moderately oversized text).

    Returns a list of (chunk_text, start_sentence, end_sentence) tuples.
    Sentence numbering is 1-based, where each paragraph = one woven sentence.
    """
    # 1. Split into paragraphs on double-newline boundaries
    raw_paragraphs = re.split(r'\n{2,}', full_text)

    # 2. Clean each paragraph: collapse internal newlines/whitespace to single spaces
    paragraphs = []
    for p in raw_paragraphs:
        cleaned = ' '.join(p.split())  # collapse all whitespace runs to single space
        if cleaned:
            paragraphs.append(cleaned)

    # 3. Pack paragraphs into chunks, respecting max_chars, tracking sentence numbers
    chunks: list[tuple[str, int, int]] = []
    current_parts: list[str] = []
    current_len = 0
    chunk_start_sentence = 1  # 1-based
    for i, para in enumerate(paragraphs):
        sentence_num = i + 1  # 1-based
        para_len = len(para)
        # If adding this paragraph would exceed max_chars, flush current chunk
        if current_parts and current_len + para_len + 1 > max_chars:
            chunks.append((' '.join(current_parts), chunk_start_sentence, sentence_num - 1))
            current_parts = []
            current_len = 0
            chunk_start_sentence = sentence_num
        current_parts.append(para)
        current_len += para_len + 1  # +1 for the joining space
    if current_parts:
        chunks.append((' '.join(current_parts), chunk_start_sentence, len(paragraphs)))

    return chunks

# --- START: MODIFIED FUNCTION (Smart Quota Handling & Multiple Voices) ---
async def generate_audio_chunk_async(
    client: Any, 
    text_chunk: str,
    chunk_index: int,
    effective_args: argparse.Namespace,
    semaphore: asyncio.Semaphore,
    temp_chunk_dir: Path,
    voice_for_this_chunk: str, # <-- NEW: Pass the specific voice for this chunk
    file_suffix: str = ""
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
                        full_prompt = f"{effective_args.tts_prompt_prefix}\n\nRead the following text exactly as written. Do not add, remove, or change any words:\n\n{text_chunk}"
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
                        temp_file_path = temp_chunk_dir / f"temp_chunk_{chunk_index:04d}{file_suffix}.wav"
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


async def generate_interleave_audio_chunk_async(
    client: Any,
    text_chunk: str,
    chunk_index: int,
    effective_args: argparse.Namespace,
    semaphore: asyncio.Semaphore,
    temp_chunk_dir: Path,
    speaker_voices: list[tuple[str, str]],
    file_suffix: str = ""
) -> Path | None:
    """Generate audio for an interleave chunk using multi-speaker TTS.

    speaker_voices is a list of (speaker_label, voice_name) tuples,
    e.g. [("Speaker 1", "Charon"), ("Speaker 2", "aoede"), ("Speaker 3", "Puck")].
    """
    try:
        async with semaphore:
            logging.info(f"[GEMINI-MULTI] Requesting multi-speaker TTS for chunk {chunk_index + 1}...")
            if not text_chunk.strip():
                silence = AudioSegment.silent(duration=100, frame_rate=PCM_FRAME_RATE)
                temp_file_path = temp_chunk_dir / f"temp_chunk_{chunk_index:04d}_silence.wav"
                silence.export(str(temp_file_path), format="wav", parameters=["-ac", str(PCM_CHANNELS), "-ar", str(PCM_FRAME_RATE)])
                return temp_file_path

            if not google_genai or not genai_types:
                raise RuntimeError("Gemini API library not available.")

            last_exception = None
            for attempt in range(effective_args.max_api_retries):
                try:
                    api_call_description = f"Chunk {chunk_index + 1} (Attempt {attempt + 1}/{effective_args.max_api_retries})"

                    prompt_text = f"{effective_args.tts_prompt_prefix}\n\nRead the following text exactly as written. Do not add, remove, or change any words:\n\n{text_chunk}"
                    contents = [
                        genai_types.Content(
                            role="user",
                            parts=[genai_types.Part.from_text(text=prompt_text)],
                        ),
                    ]

                    speaker_configs = [
                        genai_types.SpeakerVoiceConfig(
                            speaker=label,
                            voice_config=genai_types.VoiceConfig(
                                prebuilt_voice_config=genai_types.PrebuiltVoiceConfig(
                                    voice_name=voice
                                )
                            ),
                        )
                        for label, voice in speaker_voices
                    ]

                    api_config = genai_types.GenerateContentConfig(
                        temperature=1,
                        response_modalities=["audio"],
                        speech_config=genai_types.SpeechConfig(
                            multi_speaker_voice_config=genai_types.MultiSpeakerVoiceConfig(
                                speaker_voice_configs=speaker_configs,
                            ),
                        ),
                    )

                    # Use streaming to collect audio data
                    audio_buffers = []
                    async for chunk in await client.aio.models.generate_content_stream(
                        model=effective_args.model_name,
                        contents=contents,
                        config=api_config,
                    ):
                        if chunk.parts is None:
                            continue
                        part = chunk.parts[0]
                        if part.inline_data and part.inline_data.data:
                            inline_data = part.inline_data
                            data_buffer = inline_data.data
                            mime_type = inline_data.mime_type or ""
                            # Convert raw PCM to WAV if no recognized extension
                            file_extension = mimetypes.guess_extension(mime_type)
                            if file_extension is None:
                                data_buffer = convert_to_wav(data_buffer, mime_type)
                            audio_buffers.append(data_buffer)

                    if audio_buffers:
                        temp_file_path = temp_chunk_dir / f"temp_chunk_{chunk_index:04d}{file_suffix}.wav"
                        # Concatenate all streamed wav segments
                        if len(audio_buffers) == 1:
                            with open(temp_file_path, "wb") as f:
                                f.write(audio_buffers[0])
                        else:
                            combined = AudioSegment.empty()
                            for buf in audio_buffers:
                                import io
                                combined += AudioSegment.from_file(io.BytesIO(buf), format="wav")
                            combined.export(str(temp_file_path), format="wav")
                        logging.info(f"[GEMINI-MULTI] Chunk {chunk_index + 1} successfully converted and saved.")
                        return temp_file_path
                    else:
                        error_message = "No audio data in multi-speaker Gemini streaming response."
                        logging.error(f"{api_call_description} [GEMINI-MULTI] - {error_message}")
                        last_exception = RuntimeError(error_message)

                except Exception as e:
                    error_str = str(e).lower()
                    if "resource_exhausted" in error_str and "quota" in error_str:
                        logging.warning("Daily API quota has been reached. Halting all further API calls.")
                        raise QuotaExhaustedError(str(e))
                    logging.error(f"{api_call_description} [GEMINI-MULTI] - Error: {e}", exc_info=False)
                    last_exception = e

                if attempt + 1 < effective_args.max_api_retries:
                    delay = effective_args.retry_delay
                    error_str = str(last_exception).lower() if last_exception else ""
                    if any(sub in error_str for sub in ["429", "rate limit", "unavailable", "s503"]):
                        delay = effective_args.retry_delay * (2 ** attempt)
                    logging.info(f"Retrying chunk {chunk_index + 1} after error. Waiting {delay} seconds...")
                    await asyncio.sleep(delay)

            logging.error(f"[GEMINI-MULTI] Chunk {chunk_index + 1} failed after {effective_args.max_api_retries} retries. Last error: {last_exception}")
            return None
    finally:
        delay = effective_args.delay_between_chunks
        if delay > 0:
            logging.debug(f"Waiting for {delay} seconds before next chunk...")
            await asyncio.sleep(delay)


def find_sub_chunk_texts(temp_chunk_dir: Path, chunk_index: int) -> list[tuple[str, Path]]:
    """Find sub-chunk text files like temp_chunk_0004_A.txt, _B.txt, etc.
    Returns sorted list of (label, path) tuples."""
    prefix = f"temp_chunk_{chunk_index:04d}_"
    sub_chunks = []
    for f in sorted(temp_chunk_dir.iterdir()):
        if f.name.startswith(prefix) and f.suffix == '.txt':
            label = f.stem[len(prefix):]
            if re.match(r'^[A-Z]$', label):
                sub_chunks.append((label, f))
    return sub_chunks


def concatenate_sub_chunk_audio(temp_chunk_dir: Path, chunk_index: int, labels: list[str]) -> Path | None:
    """Concatenate sub-chunk wav files (_A.wav, _B.wav ...) into the main chunk wav."""
    combined = AudioSegment.empty()
    for label in labels:
        sub_wav = temp_chunk_dir / f"temp_chunk_{chunk_index:04d}_{label}.wav"
        if not sub_wav.exists() or sub_wav.stat().st_size <= 44:
            return None
        try:
            combined += AudioSegment.from_file(sub_wav)
        except Exception as e:
            logging.error(f"Error loading sub-chunk audio {sub_wav.name}: {e}")
            return None
    if len(combined) == 0:
        return None
    main_wav = temp_chunk_dir / f"temp_chunk_{chunk_index:04d}.wav"
    combined.export(str(main_wav), format="wav")
    logging.info(f"Concatenated {len(labels)} sub-chunks into {main_wav.name}")
    return main_wav


# --- START: MODIFIED FUNCTION (Voice Rotation & Robust Merging) ---
async def process_book_to_audio_async(
    text_chunks_from_file: list[str],
    client: Any,
    output_file_path: Path,
    args: argparse.Namespace,
    effective_args: argparse.Namespace,
    sentence_ranges: list[tuple[int, int]] | None = None,
    alignment_map_data: dict | None = None,
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
    
    is_interleave = getattr(args, 'interleave', False)
    logging.info("Saving source text for each chunk to the temporary directory...")
    for i, text_content in enumerate(text_chunks_from_file):
        text_file_path = temp_chunk_dir / f"temp_chunk_{i:04d}.txt"
        if (args.repair_mode or args.chunks_dir) and text_file_path.exists():
            logging.debug(f"Repair mode: Preserving existing text file {text_file_path.name}")
        else:
            try:
                if is_interleave:
                    # Preserve newlines for multi-speaker format (Speaker N: lines)
                    sanitized = text_content.replace('\r\n', '\n').replace('\r', '\n')
                else:
                    # Replace all CR/LF variants with spaces to reduce TTS error rate
                    sanitized = text_content.replace('\r\n', ' ').replace('\r', ' ').replace('\n', ' ')
                with open(text_file_path, 'w', encoding='utf-8') as f: f.write(sanitized)
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
    sub_chunk_parents = []  # (chunk_idx, [labels]) for chunks assembled from sub-parts
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
                # Check for sub-chunks (e.g. temp_chunk_0004_A.txt, _B.txt)
                sub_chunks = find_sub_chunk_texts(temp_chunk_dir, i)
                if sub_chunks:
                    logging.info(f"Repair mode: Chunk {i+1} has {len(sub_chunks)} sub-chunk(s): {', '.join(l for l, _ in sub_chunks)}")
                    for label, sc_path in sub_chunks:
                        sc_wav = temp_chunk_dir / f"temp_chunk_{i:04d}_{label}.wav"
                        sc_valid = False
                        if sc_wav.exists() and sc_wav.stat().st_size > 44:
                            try: AudioSegment.from_file(sc_wav); sc_valid = True
                            except Exception: pass
                        if not sc_valid:
                            sc_text = sc_path.read_text(encoding='utf-8').strip()
                            if sc_text:
                                tasks_to_generate.append((i, sc_text, f"_{label}"))
                                logging.info(f"  Sub-chunk {label} needs audio generation.")
                            else:
                                logging.warning(f"  Sub-chunk {label} text file is empty, skipping.")
                        else:
                            logging.info(f"  Sub-chunk {label} audio already valid.")
                    sub_chunk_parents.append((i, [l for l, _ in sub_chunks]))
                else:
                    # Read from on-disk text file (may have been hand-edited)
                    disk_txt = temp_chunk_dir / f"temp_chunk_{i:04d}.txt"
                    if disk_txt.exists():
                        disk_text = disk_txt.read_text(encoding='utf-8').strip()
                        if disk_text:
                            text_content = disk_text
                    logging.info(f"Repair mode: Chunk {i+1} needs generation.")
                    tasks_to_generate.append((i, text_content, ""))
    else: # Normal mode
        logging.info(f"--- NORMAL MODE ---")
        metadata_to_save = { "script_version": SCRIPT_VERSION, "input_filename": effective_args.input_filename, "tts_service": effective_args.tts_service, "chunk_max_chars": effective_args.chunk_max_chars, "total_expected_chunks": total_expected_chunks_count }
        if effective_args.tts_service == "gemini": metadata_to_save.update({"model_name": effective_args.model_name, "voice_name": effective_args.voice_name, "tts_prompt_prefix": effective_args.tts_prompt_prefix})
        elif effective_args.tts_service == "vertex": metadata_to_save.update({"voice_name": effective_args.voice_name, "language_code": effective_args.language_code})
        with open(metadata_file_path, 'w') as mf: json.dump(metadata_to_save, mf, indent=4)
        for i, text_content in enumerate(text_chunks_from_file):
            tasks_to_generate.append((i, text_content, ""))

    # Write per-level SF chunk metadata into this level's chunk dir.
    # Written in all modes so repair runs also stamp the meta file.
    if alignment_map_data:
        sf_chunk_meta = {
            "alignment_source_hash": alignment_map_data.get("source_hash", ""),
            "chunk_max_chars": alignment_map_data.get("chunk_max_chars", 0),
            "sentence_count": alignment_map_data.get("sentence_count", 0),
            "chunk_count": len(alignment_map_data.get("chunks", [])),
        }
        sf_meta_path = temp_chunk_dir / SF_CHUNK_META_FILENAME
        with open(sf_meta_path, 'w', encoding='utf-8') as f:
            json.dump(sf_chunk_meta, f, indent=4)
        logging.info(f"SF chunk metadata written: {sf_meta_path}")

    # Save chunk-to-sentence mapping for illustration synchronization.
    # Written in all modes (normal, repair, chunks-dir) so the map is always
    # available for video creation, even after a repair run.
    if sentence_ranges:
        chunk_map = {"chunks": [{"index": i, "start_sentence": sr[0], "end_sentence": sr[1]} for i, sr in enumerate(sentence_ranges)]}
        chunk_map_path = temp_chunk_dir / CHUNK_MAP_FILENAME
        with open(chunk_map_path, 'w') as cm: json.dump(chunk_map, cm, indent=4)
        logging.info(f"Chunk-to-sentence map saved: {chunk_map_path}")

    if tasks_to_generate:
        logging.info(f"Preparing {len(tasks_to_generate)} chunks for API calls.")
        generation_tasks_for_asyncio = []
        for original_idx, text_content, suffix in tasks_to_generate:
            # --- Assign voice for this chunk ---
            voice_for_this_chunk = voices[original_idx % len(voices)]
            generation_tasks_for_asyncio.append(
                generate_audio_chunk_async(client, text_content, original_idx, effective_args, semaphore, temp_chunk_dir, voice_for_this_chunk, suffix)
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

    # --- Assemble sub-chunk audio into parent chunks ---
    for chunk_idx, labels in sub_chunk_parents:
        result = concatenate_sub_chunk_audio(temp_chunk_dir, chunk_idx, labels)
        if result:
            logging.info(f"Chunk {chunk_idx + 1} assembled from sub-chunks {', '.join(labels)}.")
        else:
            logging.warning(f"Chunk {chunk_idx + 1}: not all sub-chunk audio ready, assembly skipped.")

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
    # API key must be set via GOOGLE_API_KEY env var (CLI args are visible in process lists).
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
    parser.add_argument("--interleave", action="store_true", help="Use multi-speaker TTS for interleaved Speaker 1/2/3 format files.")
    parser.add_argument("--output-filename", type=str, default=None, help="Override output filename (e.g. stem_V1.wav). Used for volume splits.")
    parser.add_argument("--chunk-start", type=int, default=None, help="First chunk index (0-based) for volume concat.")
    parser.add_argument("--chunk-end", type=int, default=None, help="Last chunk index (0-based, inclusive) for volume concat.")
    parser.add_argument("--force-realign", action="store_true", help="Force recreation of _sf_alignment_map.json even if audio chunks already exist. DESTRUCTIVE: invalidates cross-level SF compatibility.")
    args = parser.parse_args()

    if args.voice_name is None:
        if args.tts_service == "gemini": args.voice_name = [DEFAULT_GEMINI_VOICE_NAME]
        elif args.tts_service == "vertex": args.voice_name = [DEFAULT_VERTEX_VOICE_NAME]
    
    logging.getLogger().setLevel(args.log_level.upper()); set_library_log_levels(args.log_level)
    logging.info(f"Book to Audio Script Version: {SCRIPT_VERSION}")
    
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
                    api_key_to_use = os.getenv("GOOGLE_API_KEY")
                    if not api_key_to_use: logging.critical("GOOGLE_API_KEY environment variable not set."); return
                    client = google_genai.Client(api_key=api_key_to_use)
                    logging.info("Gemini API client configured via API key.")
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
                # Skip sub-chunk audio files (e.g., temp_chunk_0004_A.wav)
                if re.match(r'^temp_chunk_\d{4}_[A-Z]$', f.stem):
                    continue
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

        # Filter by chunk index range if --chunk-start / --chunk-end provided
        if args.chunk_start is not None or args.chunk_end is not None:
            cs = args.chunk_start if args.chunk_start is not None else 0
            ce = args.chunk_end if args.chunk_end is not None else 999999
            sorted_chunks = [c for c in sorted_chunks if cs <= get_idx(c) <= ce]
            logging.info(f"Volume range: chunks {cs}-{ce} → {len(sorted_chunks)} chunks selected")

        if not sorted_chunks:
            logging.error("No chunks in the specified range."); return

        logging.info(f"Concatenating {len(sorted_chunks)} audio chunks...")
        combined_audio = AudioSegment.empty()
        for chunk_path in sorted_chunks:
            try: combined_audio += AudioSegment.from_file(chunk_path)
            except Exception as e: logging.error(f"Error loading chunk {chunk_path}: {e}. Skipping.")
        if len(combined_audio) == 0:
            logging.error("Combined audio is empty. Final file not created."); return

        # Use --output-filename if provided, otherwise default to stem-based name
        if args.output_filename:
            final_path = output_audio_dir / args.output_filename
        else:
            final_path = output_audio_file_path
        final_path.parent.mkdir(parents=True, exist_ok=True)
        combined_audio.export(str(final_path), format=args.output_audio_format)
        logging.info(f"Successfully rebuilt audio: {final_path}")
        return
    # --- end concat-only mode ---

    raw_text = input_text_file.read_text(encoding="utf-8")

    # Auto-detect interleave mode from filename if not explicitly set
    if not args.interleave and is_interleave_file(input_text_file.name):
        logging.info(f"Auto-detected interleave file from filename: {input_text_file.name}")
        args.interleave = True

    if args.interleave:
        logging.info("Interleave mode: preserving double-newline spacing for TTS.")
        full_text = raw_text.strip()
    else:
        # Paragraph-aware chunking: chunk_text splits on double newlines
        # and cleans each paragraph internally, so pass raw text through.
        full_text = raw_text.strip()

    effective_args = argparse.Namespace(**vars(args))
    chunk_data = chunk_text(full_text, effective_args.chunk_max_chars)
    text_chunks = [c[0] for c in chunk_data]
    sentence_ranges = [(c[1], c[2]) for c in chunk_data]

    # --- SF alignment map logic (non-interleave only) ---
    alignment_map_data: dict | None = None
    if not getattr(args, 'interleave', False) and not args.concat_only:
        alignment_map_path = output_audio_dir / SF_ALIGNMENT_MAP_FILENAME
        paragraphs = split_paragraphs(full_text)
        source_hash = compute_source_hash(paragraphs)

        if not alignment_map_path.exists():
            # First run: create the canonical alignment map
            alignment_map_data = {
                "version": SF_ALIGNMENT_MAP_VERSION,
                "chunk_max_chars": effective_args.chunk_max_chars,
                "source_hash": source_hash,
                "sentence_count": len(paragraphs),
                "chunks": [
                    {"index": i, "start_sentence": s, "end_sentence": e}
                    for i, (s, e) in enumerate(sentence_ranges)
                ],
            }
            alignment_map_path.parent.mkdir(parents=True, exist_ok=True)
            with open(alignment_map_path, 'w', encoding='utf-8') as f:
                json.dump(alignment_map_data, f, indent=4)
            logging.info(f"SF alignment map created: {alignment_map_path} ({len(sentence_ranges)} chunks, {len(paragraphs)} sentences)")
        else:
            # Subsequent runs: load and validate
            if args.force_realign:
                # Check if any chunk audio exists before allowing forced realign
                chunks_root = output_audio_dir / "chunks"
                audio_exists = any(
                    wav.exists()
                    for d in (chunks_root.iterdir() if chunks_root.exists() else [])
                    if d.is_dir()
                    for wav in d.glob("temp_chunk_*.wav")
                )
                if audio_exists:
                    logging.warning(
                        "--force-realign: deleting existing _sf_alignment_map.json. "
                        "Existing level audio chunks are now incompatible with the new alignment. "
                        "Regenerate all affected levels before running SF assembly."
                    )
                alignment_map_path.unlink(missing_ok=True)
                # Recreate
                alignment_map_data = {
                    "version": SF_ALIGNMENT_MAP_VERSION,
                    "chunk_max_chars": effective_args.chunk_max_chars,
                    "source_hash": source_hash,
                    "sentence_count": len(paragraphs),
                    "chunks": [
                        {"index": i, "start_sentence": s, "end_sentence": e}
                        for i, (s, e) in enumerate(sentence_ranges)
                    ],
                }
                alignment_map_path.parent.mkdir(parents=True, exist_ok=True)
                with open(alignment_map_path, 'w', encoding='utf-8') as f:
                    json.dump(alignment_map_data, f, indent=4)
                logging.info(f"SF alignment map recreated (--force-realign): {alignment_map_path}")
            else:
                with open(alignment_map_path, 'r', encoding='utf-8') as f:
                    alignment_map_data = json.load(f)
            saved_hash = alignment_map_data.get("source_hash", "")
            saved_chars = alignment_map_data.get("chunk_max_chars", 0)
            saved_count = alignment_map_data.get("sentence_count", 0)
            if saved_hash != source_hash:
                logging.warning(
                    f"SF alignment map source_hash mismatch! Saved={saved_hash[:20]}... Current={source_hash[:20]}... "
                    f"Sentence count: saved={saved_count}, current={len(paragraphs)}. "
                    "Continuing with existing map — audio may be misaligned if sentence count changed."
                )
            if saved_chars != effective_args.chunk_max_chars:
                logging.warning(
                    f"SF alignment map chunk_max_chars mismatch: saved={saved_chars}, current={effective_args.chunk_max_chars}. "
                    "Using saved boundaries from existing map."
                )
            # Rechunk text using saved boundaries so all levels have identical splits
            rechunked = rechunk_by_alignment_map(paragraphs, alignment_map_data["chunks"])
            text_chunks = [c[0] for c in rechunked]
            sentence_ranges = [(c[1], c[2]) for c in rechunked]
            logging.info(f"SF alignment map loaded: {alignment_map_path} ({len(text_chunks)} chunks)")
    # --- end SF alignment map logic ---

    start_time = time.time()
    
    # --- NEW: Graceful exit for quota errors ---
    try:
        await process_book_to_audio_async(text_chunks, client, output_audio_file_path, args, effective_args, sentence_ranges, alignment_map_data)
    except QuotaExhaustedError:
        logging.info("Process stopped gracefully due to API quota exhaustion. You can resume tomorrow by re-running in --repair-mode.")
        sys.exit(0) # Exit with success code, as this is an expected stop condition.
    # --- END NEW ---
    
    logging.info(f"Total processing time: {time.time() - start_time:.2f} seconds.")
# --- END MODIFIED FUNCTION ---

if __name__ == "__main__":
    asyncio.run(main_async())