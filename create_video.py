import argparse
import json
import tomllib
import subprocess
import math
from pathlib import Path
from itertools import cycle
from pydub import AudioSegment

# --- Configuration (unchanged) ---
DEFAULT_FRAME_RATE = 1
DEFAULT_IMAGE_DURATION_SECONDS = 60


def compute_illustration_durations_proportional(
    illustrations_dir: Path,
    audio_duration_seconds: float,
) -> list[tuple[Path, float]] | None:
    """Compute per-illustration durations by proportional sentence count.

    Used when a _chunk_map.json is unavailable (e.g. assembled SF audio).
    Distributes the total audio duration across illustrations in proportion to
    the number of sentences each illustration covers.

    Returns list of (image_path, duration_seconds) or None if mapping is missing.
    """
    illust_map_path = illustrations_dir / "_illustration_map.json"
    if not illust_map_path.exists():
        return None

    with open(illust_map_path) as f:
        illust_map = json.load(f)

    illustrations = illust_map.get("illustrations", [])
    if not illustrations:
        return None

    entries: list[tuple[Path, int]] = []
    for illust in illustrations:
        img_path = illustrations_dir / illust["file"]
        if not img_path.exists():
            continue
        sentence_count = illust["end_sentence"] - illust["start_sentence"] + 1
        entries.append((img_path, sentence_count))

    if not entries:
        return None

    total_sentences = sum(c for _, c in entries)
    if total_sentences == 0:
        return None

    return [(p, audio_duration_seconds * (c / total_sentences)) for p, c in entries]


def compute_illustration_durations(
    chunks_dir: Path,
    illustrations_dir: Path,
) -> list[tuple[Path, float]] | None:
    """Compute per-illustration durations from sentence mappings and chunk audio.

    Reads _chunk_map.json (from TTS chunking) and _illustration_map.json (from
    prompt generation). For each illustration, sums the durations of all audio
    chunks whose sentences majority-overlap that illustration's sentence range.

    Returns list of (image_path, duration_seconds) or None if mapping files
    are missing or no usable data is found.
    """
    chunk_map_path = chunks_dir / "_chunk_map.json"
    illust_map_path = illustrations_dir / "_illustration_map.json"

    if not chunk_map_path.exists() or not illust_map_path.exists():
        return None

    with open(chunk_map_path) as f:
        chunk_map = json.load(f)
    with open(illust_map_path) as f:
        illust_map = json.load(f)

    # Get audio duration for each chunk
    chunk_durations: dict[int, float] = {}
    for chunk_info in chunk_map["chunks"]:
        idx = chunk_info["index"]
        wav_path = chunks_dir / f"temp_chunk_{idx:04d}.wav"
        silence_path = chunks_dir / f"temp_chunk_{idx:04d}_silence.wav"
        audio_path = wav_path if wav_path.exists() else (silence_path if silence_path.exists() else None)
        if audio_path:
            try:
                audio = AudioSegment.from_file(audio_path)
                chunk_durations[idx] = len(audio) / 1000.0
            except Exception:
                chunk_durations[idx] = 0.0
        else:
            chunk_durations[idx] = 0.0

    illustrations = illust_map.get("illustrations", [])
    if not illustrations:
        return None

    parsed_illustrations: list[dict] = []
    duration_by_image: dict[Path, float] = {}
    for illust in illustrations:
        img_file = illust["file"]
        img_path = illustrations_dir / img_file
        if not img_path.exists():
            continue
        parsed = {
            "path": img_path,
            "start": illust["start_sentence"],
            "end": illust["end_sentence"],
        }
        parsed_illustrations.append(parsed)
        duration_by_image[img_path] = 0.0

    if not parsed_illustrations:
        return None

    # Allocate each chunk's duration proportionally by sentence overlap. This
    # avoids dropping long chunks that straddle multiple illustration ranges.
    for chunk_info in chunk_map.get("chunks", []):
        c_idx = chunk_info["index"]
        c_start = chunk_info["start_sentence"]
        c_end = chunk_info["end_sentence"]
        c_duration = chunk_durations.get(c_idx, 0.0)

        if c_duration <= 0.0 or c_end < c_start:
            continue

        chunk_sentences = c_end - c_start + 1
        weighted_overlaps: list[tuple[Path, float]] = []

        for ill in parsed_illustrations:
            overlap_start = max(c_start, ill["start"])
            overlap_end = min(c_end, ill["end"])
            if overlap_start > overlap_end:
                continue
            overlap_count = overlap_end - overlap_start + 1
            weight = overlap_count / chunk_sentences
            weighted_overlaps.append((ill["path"], weight))

        if weighted_overlaps:
            total_weight = sum(w for _, w in weighted_overlaps)
            if total_weight > 0.0:
                for img_path, weight in weighted_overlaps:
                    duration_by_image[img_path] += c_duration * (weight / total_weight)
        else:
            # If a chunk falls outside mapped ranges (common off-by-one at tail),
            # attach it to the last illustration so audio coverage is preserved.
            last_img = parsed_illustrations[-1]["path"]
            duration_by_image[last_img] += c_duration

    result: list[tuple[Path, float]] = []
    for ill in parsed_illustrations:
        img_path = ill["path"]
        total_duration = duration_by_image.get(img_path, 0.0)
        if total_duration > 0.0:
            result.append((img_path, total_duration))

    return result if result else None

def create_video_from_audio(
    audio_file: Path,
    illustration_files: list[Path],
    output_dir: Path,
    frame_rate: int,
    image_duration: int,
    chunks_dir: Path | None = None,
    illustrations_dir: Path | None = None
):
    """
    Creates a video for a single audio file using illustrations.

    If chunks_dir and illustrations_dir are provided and both contain mapping
    files (_chunk_map.json / _illustration_map.json), variable per-image
    durations are computed from actual chunk audio lengths. Otherwise, falls
    back to fixed image_duration cycling.
    """
    if not illustration_files:
        print(f"  [ERROR] No illustrations found. Cannot create video for '{audio_file.name}'.")
        return

    print(f"\n--- Processing: {audio_file.name} ---")

    # 1. Get audio duration (unchanged)
    try:
        audio = AudioSegment.from_file(audio_file)
        audio_duration_seconds = len(audio) / 1000.0
        print(f"  -> Audio duration: {audio_duration_seconds:.2f} seconds")
    except Exception as e:
        print(f"  [ERROR] Could not read audio file '{audio_file.name}'. Skipping. Error: {e}")
        return

    # 2. Build timeline entries for slideshow rendering.
    variable_durations = None
    if chunks_dir and illustrations_dir:
        variable_durations = compute_illustration_durations(chunks_dir, illustrations_dir)

    if variable_durations is None and illustrations_dir is not None:
        variable_durations = compute_illustration_durations_proportional(illustrations_dir, audio_duration_seconds)
        if variable_durations:
            print(f"  -> No chunk map found; using sentence-proportional illustration durations ({len(variable_durations)} images).")

    if variable_durations:
        print(f"  -> Using sentence-synchronized illustration durations ({len(variable_durations)} images).")
        timeline_entries = [(p, d) for p, d in variable_durations if d > 0.0]
        total_timeline_duration = sum(d for _, d in timeline_entries)
        print(
            f"  -> Timeline prepared with {len(timeline_entries)} variable-duration entries "
            f"(scheduled {total_timeline_duration:.2f}s)."
        )
    else:
        print(f"  -> Creating fixed-duration slideshow timeline (cycling, {image_duration}s per image)...")
        num_full_segments = math.floor(audio_duration_seconds / image_duration)
        remaining_seconds = audio_duration_seconds % image_duration

        image_cycler = cycle(illustration_files)
        timeline_entries: list[tuple[Path, float]] = []
        for _ in range(num_full_segments):
            image_path = next(image_cycler)
            timeline_entries.append((image_path, float(image_duration)))

        if remaining_seconds > 0:
            image_path = next(image_cycler)
            timeline_entries.append((image_path, float(remaining_seconds)))

        print(f"  -> Timeline prepared with {len(timeline_entries)} entries.")

    if not timeline_entries:
        print("  [ERROR] No timeline entries with positive duration. Skipping.")
        return

    # 3. Construct and run the FFmpeg command.
    output_video_path = output_dir / f"{audio_file.stem}.mp4"

    # Write a concat demuxer file so the FFmpeg command line stays short
    # regardless of how many image entries are in the timeline (avoids
    # WinError 206 "filename or extension is too long" on large SF files).
    concat_list_path = output_dir / f"{audio_file.stem}_concat_list.txt"
    with concat_list_path.open('w', encoding='utf-8') as cf:
        for image_path, dur in timeline_entries:
            # ffconcat paths must use forward slashes and be escaped
            safe_path = str(image_path).replace('\\', '/').replace("'", "\\'")
            cf.write(f"file '{safe_path}'\n")
            cf.write(f"duration {dur:.3f}\n")
        # Repeat last frame so the final image duration is honoured
        if timeline_entries:
            safe_path = str(timeline_entries[-1][0]).replace('\\', '/').replace("'", "\\'")
            cf.write(f"file '{safe_path}'\n")

    # Use the fps filter (BEFORE scale) to materialize a constant-frame-rate
    # video stream from the concat demuxer's still-image inputs. Without an
    # explicit fps filter, libx264 + concat-of-stills drops frames at segment
    # boundaries and the video ends short of the scheduled timeline (observed
    # ~57s truncation on a 642s slideshow). The fps filter expands each PNG
    # segment to `frame_rate` frames per second of duration, which preserves
    # the full timeline. We then clamp output to the audio length via -t and
    # drop -shortest (no longer needed; video stream already matches audio).
    command = [
        'ffmpeg', '-y',
        '-f', 'concat', '-safe', '0', '-i', str(concat_list_path),
        '-i', str(audio_file),
        '-vf', f'fps={frame_rate},scale=1280:-2,setsar=1',
        '-map', '0:v:0',
        '-map', '1:a:0',
        '-c:v', 'libx264',
        '-tune', 'stillimage',
        '-threads', '4',
        '-c:a', 'aac',
        '-b:a', '192k',
        '-pix_fmt', 'yuv420p',
        '-t', f'{audio_duration_seconds:.3f}',
        str(output_video_path)
    ]

    print(f"  -> Running FFmpeg command (concat demuxer, {len(timeline_entries)} entries)...")
    result = subprocess.run(command, capture_output=True, text=True)

    # Remove the temporary concat list regardless of outcome
    try:
        concat_list_path.unlink(missing_ok=True)
    except Exception:
        pass

    if result.returncode != 0:
        print(f"  [ERROR] FFmpeg failed for '{audio_file.name}'.")
        print("  -> FFmpeg stderr:")
        print(result.stderr)
    else:
        print(f"  -> Successfully created video: {output_video_path.name}")


def main():
    parser = argparse.ArgumentParser(description="Create audiobook-style videos from audio files and illustrations.")
    parser.add_argument("book_name", nargs='?', type=str, default=None, help="The name of the book directory inside the 'video' folder (legacy mode).")
    parser.add_argument("--audio-file", type=Path, default=None, help="Full path to a single audio file (overrides book_name mode).")
    parser.add_argument("--illustrations-dir", type=Path, default=None, help="Full path to the illustrations directory.")
    parser.add_argument("--output-dir", type=Path, default=None, help="Full path to the video output directory.")
    parser.add_argument("--chunks-dir", type=Path, default=None, help="Path to TTS chunks directory (for sentence-synchronized durations).")
    parser.add_argument("--frame-rate", type=int, default=DEFAULT_FRAME_RATE, help="Output video frame rate.")
    parser.add_argument("--image-duration", type=int, default=DEFAULT_IMAGE_DURATION_SECONDS, help="Seconds per illustration.")
    args = parser.parse_args()

    if args.audio_file and args.illustrations_dir and args.output_dir:
        # Explicit path mode: process a single audio file with provided paths
        if not args.audio_file.exists():
            print(f"Error: Audio file not found at '{args.audio_file}'"); return
        if not args.illustrations_dir.is_dir():
            print(f"Error: Illustrations directory not found at '{args.illustrations_dir}'"); return

        args.output_dir.mkdir(parents=True, exist_ok=True)

        illustration_files = sorted([p for p in args.illustrations_dir.iterdir() if p.is_file() and p.suffix.lower() in ['.png', '.jpg', '.jpeg']])
        if not illustration_files:
            print(f"No illustrations found in '{args.illustrations_dir}'. Cannot create video."); return

        print(f"Processing single file with {len(illustration_files)} illustration(s).")
        print(f"Video will be saved to: {args.output_dir}")

        create_video_from_audio(
            args.audio_file,
            illustration_files,
            args.output_dir,
            args.frame_rate,
            args.image_duration,
            chunks_dir=args.chunks_dir,
            illustrations_dir=args.illustrations_dir
        )
        print("\n--- Video processed. ---")

    elif args.book_name:
        # Legacy mode: resolve from config.toml
        try:
            with open("config.toml", "rb") as f: config = tomllib.load(f)
            content_project_dir_str = config.get("content_project_dir")
            if not content_project_dir_str: raise ValueError("'content_project_dir' not found in config.")
            content_project_root = Path(content_project_dir_str)
        except (IOError, ValueError) as e:
            print(f"Error loading config.toml: {e}"); return

        video_root = content_project_root / "video"
        book_dir = video_root / args.book_name
        audio_dir = book_dir / "audio"
        illustrations_dir = book_dir / "illustrations"
        output_dir = book_dir / "output"

        if not book_dir.is_dir(): print(f"Error: Book directory not found at '{book_dir}'"); return
        if not audio_dir.is_dir(): print(f"Error: Audio directory not found at '{audio_dir}'"); return
        if not illustrations_dir.is_dir(): print(f"Error: Illustrations directory not found at '{illustrations_dir}'"); return

        output_dir.mkdir(exist_ok=True)

        audio_files = sorted([p for p in audio_dir.iterdir() if p.is_file() and p.suffix.lower() in ['.wav', '.mp3', '.flac']])
        illustration_files = sorted([p for p in illustrations_dir.iterdir() if p.is_file() and p.suffix.lower() in ['.png', '.jpg', '.jpeg']])

        if not audio_files: print(f"No audio files found in '{audio_dir}'. Nothing to do."); return
        if not illustration_files: print(f"No illustrations found in '{illustrations_dir}'. Cannot create videos."); return

        print(f"Found {len(audio_files)} audio file(s) and {len(illustration_files)} illustration(s).")
        print(f"Videos will be saved to: {output_dir}")

        for audio_path in audio_files:
            create_video_from_audio(
                audio_path,
                illustration_files,
                output_dir,
                args.frame_rate,
                args.image_duration
            )

        print("\n--- All videos processed. ---")
    else:
        print("Error: Provide either --audio-file + --illustrations-dir + --output-dir, or a positional book_name.")
        parser.print_help()

if __name__ == "__main__":
    main()