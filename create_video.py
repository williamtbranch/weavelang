import argparse
import tomllib
import subprocess
import shutil
import math
from pathlib import Path
from itertools import cycle
from pydub import AudioSegment

# --- Configuration (unchanged) ---
DEFAULT_FRAME_RATE = 1
DEFAULT_IMAGE_DURATION_SECONDS = 60
TEMP_DIR_NAME = "_temp_ffmpeg_files" # Renamed for clarity

def create_video_from_audio(
    audio_file: Path,
    illustration_files: list[Path],
    output_dir: Path,
    frame_rate: int,
    image_duration: int
):
    """
    Creates a video for a single audio file using a cycling list of illustrations.
    (V3: Uses a smart concat manifest to avoid file limits and improve performance).
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

    # 2. Prepare temporary directory (now only for the manifest file)
    temp_dir = output_dir / TEMP_DIR_NAME
    if temp_dir.exists():
        shutil.rmtree(temp_dir)
    temp_dir.mkdir()
    print(f"  -> Created temporary directory: {temp_dir}")

    # --- START OF DEFINITIVE FIX ---

    # 3. Generate the SMART manifest file (no longer copying frames)
    manifest_path = temp_dir / "manifest.txt"
    print(f"  -> Creating smart FFmpeg manifest file (this will be very fast)...")
    
    num_full_segments = math.floor(audio_duration_seconds / image_duration)
    remaining_seconds = audio_duration_seconds % image_duration
    
    image_cycler = cycle(illustration_files)

    with open(manifest_path, "w", encoding="utf-8") as f:
        # Write entries for the full 60-second segments
        for _ in range(num_full_segments):
            image_path = next(image_cycler)
            # FFmpeg needs forward slashes and proper quoting for paths with spaces
            safe_path = str(image_path.resolve()).replace('\\', '/')
            f.write(f"file '{safe_path}'\n")
            f.write(f"duration {image_duration}\n")

        # Write the final entry for the remaining duration
        if remaining_seconds > 0:
            image_path = next(image_cycler)
            safe_path = str(image_path.resolve()).replace('\\', '/')
            f.write(f"file '{safe_path}'\n")
            f.write(f"duration {remaining_seconds}\n")

    print(f"  -> Manifest generated with {num_full_segments + (1 if remaining_seconds > 0 else 0)} entries.")

    # 4. Construct and run the FFmpeg command (this is the same as the last version)
    output_video_path = output_dir / f"{audio_file.stem}.mp4"
    command = [
        'ffmpeg',
        '-f', 'concat',
        '-i', str(manifest_path),
        '-i', str(audio_file),
        '-vf', 'scale=1280:-2',
        '-c:v', 'libx264',
        '-tune', 'stillimage',
        '-threads', '4',
        '-c:a', 'aac',
        '-b:a', '192k',
        '-pix_fmt', 'yuv420p',
        '-r', str(frame_rate), # Set output frame rate
        '-shortest',
        str(output_video_path)
    ]
    # --- END OF DEFINITIVE FIX ---

    print(f"  -> Running FFmpeg command...")
    # Use -y to automatically overwrite existing output file for convenience
    command.insert(1, '-y') 
    result = subprocess.run(command, capture_output=True, text=True)

    if result.returncode != 0:
        print(f"  [ERROR] FFmpeg failed for '{audio_file.name}'.")
        print("  -> FFmpeg stderr:")
        print(result.stderr)
    else:
        print(f"  -> Successfully created video: {output_video_path.name}")

    # 5. Clean up temporary directory (unchanged)
    shutil.rmtree(temp_dir)
    print(f"  -> Cleaned up temporary directory.")


def main():
    parser = argparse.ArgumentParser(description="Create audiobook-style videos from audio files and illustrations.")
    parser.add_argument("book_name", nargs='?', type=str, default=None, help="The name of the book directory inside the 'video' folder (legacy mode).")
    parser.add_argument("--audio-file", type=Path, default=None, help="Full path to a single audio file (overrides book_name mode).")
    parser.add_argument("--illustrations-dir", type=Path, default=None, help="Full path to the illustrations directory.")
    parser.add_argument("--output-dir", type=Path, default=None, help="Full path to the video output directory.")
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
            args.image_duration
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