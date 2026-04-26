"""sf_assemble.py — Study Format audio assembly.

Reads the canonical _sf_alignment_map.json produced by book_to_audio.py and
interlaces per-level TTS chunks into a single study-format WAV file.

Output file:  <output_dir>/<book_name>ULsf.wav
Reference txt: <output_dir>/<book_name>ULsf.txt

Usage:
    python sf_assemble.py \
        --book-dir PATH \
        --book-name NAME \
        [--source-level r] \
        [--levels 16,19,22,25,28,31,34] \
        [--gap-intra-ms 150] \
        [--gap-inter-ms 350] \
        [--output-dir PATH]  (defaults to book-dir/audio) \
        [--output-format wav]
"""

import argparse
import json
import logging
import struct
import sys
import wave
from pathlib import Path

# ---------------------------------------------------------------------------
# Constants (must match book_to_audio.py)
# ---------------------------------------------------------------------------

SF_ALIGNMENT_MAP_FILENAME = "_sf_alignment_map.json"
SF_CHUNK_META_FILENAME = "_sf_chunk_meta.json"
CHUNK_GLOB = "temp_chunk_*.wav"

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s",
    datefmt="%H:%M:%S",
)
log = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# WAV helpers
# ---------------------------------------------------------------------------

def read_wav_bytes(path: Path) -> tuple[bytes, int, int, int]:
    """Read a WAV file and return (pcm_data, n_channels, sample_width, framerate)."""
    with wave.open(str(path), "rb") as wf:
        params = wf.getparams()
        data = wf.readframes(params.nframes)
    return data, params.nchannels, params.sampwidth, params.framerate


def make_silence(duration_ms: int, n_channels: int, sample_width: int, framerate: int) -> bytes:
    """Return PCM silence of the given duration."""
    n_frames = int(framerate * duration_ms / 1000)
    return b"\x00" * (n_frames * n_channels * sample_width)


def write_wav(path: Path, pcm: bytes, n_channels: int, sample_width: int, framerate: int) -> None:
    with wave.open(str(path), "wb") as wf:
        wf.setnchannels(n_channels)
        wf.setsampwidth(sample_width)
        wf.setframerate(framerate)
        wf.writeframes(pcm)


# ---------------------------------------------------------------------------
# Chunk-dir discovery
# ---------------------------------------------------------------------------

def find_chunk_dir(chunks_root: Path, book_name: str, level_suffix: str) -> Path | None:
    """
    Resolve the chunk directory for a given level suffix (e.g. 'r', '22').
    Suffix maps to directory name pattern: <book_name>UL<suffix>.

    Prefers exact match; falls back to any dir ending with 'UL<suffix>'.
    """
    suffix = f"UL{level_suffix}"
    preferred = chunks_root / f"{book_name}{suffix}"
    if preferred.is_dir():
        return preferred
    # Fallback scan
    for d in chunks_root.iterdir():
        if d.is_dir() and d.name.endswith(suffix):
            return d
    return None


def load_chunk_wavs(chunk_dir: Path, chunk_count: int) -> list[Path | None]:
    """
    Return an ordered list of WAV paths for chunk indices 0..chunk_count-1.
    Entries are None for any missing chunk.
    Chunks are named temp_chunk_<index>.wav (0-based index).
    """
    result: list[Path | None] = []
    for i in range(chunk_count):
        p = chunk_dir / f"temp_chunk_{i:04d}.wav"
        result.append(p if p.exists() else None)
    return result


# ---------------------------------------------------------------------------
# Validation
# ---------------------------------------------------------------------------

def validate_chunk_meta(chunk_dir: Path, alignment_map: dict, level_label: str) -> bool:
    """
    Load _sf_chunk_meta.json from chunk_dir and cross-check against the
    canonical alignment map.  Returns True on success, False on mismatch.
    """
    meta_path = chunk_dir / SF_CHUNK_META_FILENAME
    if not meta_path.exists():
        log.warning("[%s] _sf_chunk_meta.json not found in %s — skipping hash check", level_label, chunk_dir)
        return True  # Non-fatal: allow assembly without meta

    with meta_path.open(encoding="utf-8") as f:
        meta = json.load(f)

    canonical_hash = alignment_map.get("source_hash", "")
    meta_hash = meta.get("alignment_source_hash", "")
    if canonical_hash and meta_hash and canonical_hash != meta_hash:
        log.error(
            "[%s] Alignment hash mismatch! Canonical=%s... Meta=%s... "
            "This level's audio was generated from a different text version. "
            "Regenerate audio for this level before assembling SF.",
            level_label, canonical_hash[:20], meta_hash[:20],
        )
        return False

    canonical_chunk_count = len(alignment_map.get("chunks", []))
    meta_chunk_count = meta.get("chunk_count", 0)
    if meta_chunk_count and meta_chunk_count != canonical_chunk_count:
        log.error(
            "[%s] Chunk count mismatch: canonical=%d, meta=%d. "
            "Regenerate audio for this level.",
            level_label, canonical_chunk_count, meta_chunk_count,
        )
        return False

    return True


# ---------------------------------------------------------------------------
# Assembly
# ---------------------------------------------------------------------------

def assemble(
    alignment_map: dict,
    level_order: list[str],     # e.g. ["r", "16", "19", ...]
    chunk_wav_lists: dict[str, list[Path | None]],  # level_suffix -> ordered wav paths
    gap_intra_ms: int,
    gap_inter_ms: int,
    require_all_chunks: bool = True,
) -> tuple[bytes, int, int, int, list[str]]:
    """
    Interlace chunk audio in study-format order:
        [chunk 0 source] [intra gap] [chunk 0 lv16] [intra gap] ... [inter gap]
        [chunk 1 source] ...

    Returns (pcm_bytes, n_channels, sample_width, framerate, reference_lines).
    reference_lines is a list of text strings for the .txt reference file.
    """
    chunks_meta = alignment_map["chunks"]
    n_chunks = len(chunks_meta)

    # Determine audio params from the first available WAV
    n_channels = sample_width = framerate = None
    for level in level_order:
        for wav_path in chunk_wav_lists.get(level, []):
            if wav_path and wav_path.exists():
                _, n_channels, sample_width, framerate = read_wav_bytes(wav_path)
                break
        if framerate is not None:
            break

    if framerate is None:
        raise RuntimeError("No valid WAV files found — cannot determine audio parameters.")

    intra_silence = make_silence(gap_intra_ms, n_channels, sample_width, framerate)
    inter_silence = make_silence(gap_inter_ms, n_channels, sample_width, framerate)

    output_pcm = bytearray()
    reference_lines: list[str] = []

    for chunk_idx, chunk_def in enumerate(chunks_meta):
        group_had_audio = False
        for level_idx, level in enumerate(level_order):
            wav_paths = chunk_wav_lists.get(level, [])
            wav_path = wav_paths[chunk_idx] if chunk_idx < len(wav_paths) else None

            if wav_path is None or not wav_path.exists():
                if require_all_chunks:
                    raise RuntimeError(
                        f"Missing chunk {chunk_idx} for level '{level}': {wav_path}. "
                        "Use --no-require-all to allow gaps."
                    )
                log.warning("Skipping missing chunk %d for level '%s'", chunk_idx, level)
                continue

            pcm, ch, sw, fr = read_wav_bytes(wav_path)
            if (ch, sw, fr) != (n_channels, sample_width, framerate):
                raise RuntimeError(
                    f"Audio params mismatch for chunk {chunk_idx} level '{level}': "
                    f"expected ({n_channels}ch, {sample_width}B, {framerate}Hz), "
                    f"got ({ch}ch, {sw}B, {fr}Hz)."
                )

            # Intra-gap before each variant except the very first in the group
            if group_had_audio:
                output_pcm.extend(intra_silence)

            output_pcm.extend(pcm)
            group_had_audio = True

            # Reference label
            label = "source" if level_idx == 0 else f"lv{level}"
            reference_lines.append(
                f"[chunk {chunk_idx+1}/{n_chunks}] [{label}] "
                f"sentences {chunk_def.get('start_sentence','?')}-{chunk_def.get('end_sentence','?')}"
            )

        # Inter-gap after each chunk group (including last, for padding)
        if group_had_audio:
            output_pcm.extend(inter_silence)

    return bytes(output_pcm), n_channels, sample_width, framerate, reference_lines


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(description="Assemble study-format audio from level audio chunks.")
    parser.add_argument("--book-dir", required=True, type=Path, help="Book/chapter directory containing audio/ subfolder and _av_manifest.toml.")
    parser.add_argument("--book-name", required=True, type=str, help="Base stem name, e.g. MobyDickCH1. Used to locate chunk dirs.")
    parser.add_argument("--source-level", default="r", help="Level suffix for the source-language audio (default: r).")
    parser.add_argument("--levels", default="16,19,22,25,28,31,34", help="Comma-separated ordered level suffixes to interlace (default: 16,19,22,25,28,31,34).")
    parser.add_argument("--gap-intra-ms", type=int, default=150, help="Silence gap in ms between level variants within one chunk group (default: 150).")
    parser.add_argument("--gap-inter-ms", type=int, default=350, help="Silence gap in ms between chunk groups (default: 350).")
    parser.add_argument("--output-dir", type=Path, default=None, help="Output directory (default: book-dir/audio).")
    parser.add_argument("--output-format", default="wav", help="Output audio format (default: wav; only wav supported currently).")
    parser.add_argument("--no-require-all", action="store_true", help="Continue even if some chunk WAVs are missing (gaps become silence).")
    args = parser.parse_args()

    book_dir = args.book_dir.resolve()
    if not book_dir.is_dir():
        log.error("--book-dir does not exist: %s", book_dir)
        sys.exit(1)

    audio_dir = book_dir / "audio"
    alignment_map_path = audio_dir / SF_ALIGNMENT_MAP_FILENAME
    if not alignment_map_path.exists():
        log.error(
            "Alignment map not found: %s\n"
            "Run audio generation for any level first to create it.",
            alignment_map_path,
        )
        sys.exit(1)

    with alignment_map_path.open(encoding="utf-8") as f:
        alignment_map = json.load(f)

    chunk_count = len(alignment_map.get("chunks", []))
    if chunk_count == 0:
        log.error("Alignment map contains no chunks: %s", alignment_map_path)
        sys.exit(1)

    log.info("Alignment map loaded: %d chunks, source_hash=%s...", chunk_count, alignment_map.get("source_hash","")[:16])

    # Build ordered level list: source first, then study levels
    level_order = [args.source_level] + [lv.strip() for lv in args.levels.split(",") if lv.strip()]
    chunks_root = audio_dir / "chunks"

    # Resolve and validate chunk dirs
    chunk_wav_lists: dict[str, list[Path | None]] = {}
    errors: list[str] = []
    for level in level_order:
        chunk_dir = find_chunk_dir(chunks_root, args.book_name, level)
        if chunk_dir is None:
            errors.append(f"No chunk directory found for level '{level}' (looked for UL{level} under {chunks_root})")
            continue

        if not validate_chunk_meta(chunk_dir, alignment_map, level):
            errors.append(f"Level '{level}' failed alignment validation (see warnings above).")
            continue

        chunk_wav_lists[level] = load_chunk_wavs(chunk_dir, chunk_count)
        missing = sum(1 for p in chunk_wav_lists[level] if p is None or not p.exists())
        log.info("  Level %-6s → %s  (%d chunks, %d missing)", level, chunk_dir.name, chunk_count, missing)
        if missing and not args.no_require_all:
            errors.append(f"Level '{level}': {missing}/{chunk_count} chunk WAVs missing.")

    if errors:
        for err in errors:
            log.error(err)
        log.error("SF assembly aborted. Fix the above issues or use --no-require-all to skip missing chunks.")
        sys.exit(1)

    # Assemble
    log.info(
        "Assembling SF audio: %d levels × %d chunks, intra=%dms, inter=%dms",
        len(level_order), chunk_count, args.gap_intra_ms, args.gap_inter_ms,
    )
    try:
        pcm, n_channels, sample_width, framerate, reference_lines = assemble(
            alignment_map=alignment_map,
            level_order=level_order,
            chunk_wav_lists=chunk_wav_lists,
            gap_intra_ms=args.gap_intra_ms,
            gap_inter_ms=args.gap_inter_ms,
            require_all_chunks=not args.no_require_all,
        )
    except RuntimeError as exc:
        log.error("Assembly failed: %s", exc)
        sys.exit(1)

    output_dir = (args.output_dir or audio_dir).resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    out_stem = f"{args.book_name}ULsf"
    out_wav = output_dir / f"{out_stem}.wav"
    out_txt = output_dir / f"{out_stem}.txt"

    write_wav(out_wav, pcm, n_channels, sample_width, framerate)
    out_txt.write_text("\n".join(reference_lines) + "\n", encoding="utf-8")

    duration_s = len(pcm) / (n_channels * sample_width * framerate)
    log.info(
        "SF assembly complete: %s (%.1fs, %d bytes)",
        out_wav, duration_s, len(pcm),
    )
    log.info("Reference text:       %s", out_txt)


if __name__ == "__main__":
    main()
