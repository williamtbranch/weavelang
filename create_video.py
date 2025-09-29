import argparse
import tomllib
import subprocess
import shutil
import math
from pathlib import Path
from itertools import cycle
from pydub import AudioSegment

# --- Configuration ---
DEFAULT_FRAME_RATE = 1  # 1 frame per second is standard and works well on YouTube.
DEFAULT_IMAGE_DURATION_SECONDS = 60 # Each illustration will be shown for this long.
TEMP_FRAME_DIR = "_temp_frames"

def create_video_from_audio(
    audio_file: Path,
    illustration_files: list[Path],
    output_dir: Path,
    frame_rate: int,
    image_duration: int
):
    """
    Creates a video for a single audio file using a cycling list of illustrations.
    """
    if not illustration_files:
        print(f"  [ERROR] No illustrations found. Cannot create video for '{audio_file.name}'.")
        return

    print(f"\n--- Processing: {audio_file.name} ---")

    # 1. Get audio duration
    try:
        audio = AudioSegment.from_file(audio_file)
        audio_duration_seconds = len(audio) / 1000.0
        print(f"  -> Audio duration: {audio_duration_seconds:.2f} seconds")
    except Exception as e:
        print(f"  [ERROR] Could not read audio file '{audio_file.name}'. Skipping. Error: {e}")
        return

    # 2. Prepare temporary directory for video frames
    temp_dir = output_dir / TEMP_FRAME_DIR
    if temp_dir.exists():
        shutil.rmtree(temp_dir)
    temp_dir.mkdir()
    print(f"  -> Created temporary frame directory: {temp_dir}")

    # 3. Generate the image sequence
    total_frames = math.ceil(audio_duration_seconds * frame_rate)
    frames_per_image = image_duration * frame_rate
    
    print(f"  -> Generating {total_frames} frames (image will change every {frames_per_image} frames)...")
    
    image_cycler = cycle(illustration_files)
    current_image = next(image_cycler)
    
    for i in range(total_frames):
        if i > 0 and i % frames_per_image == 0:
            current_image = next(image_cycler)
        
        # Create a copy of the current image for this frame.
        # Using copy is more portable than symlinks on Windows.
        frame_filename = temp_dir / f"frame_{i:05d}{current_image.suffix}"
        shutil.copy(current_image, frame_filename)

    print("  -> Frame sequence generated.")

    # 4. Construct and run the FFmpeg command
    output_video_path = output_dir / f"{audio_file.stem}.mp4"
    image_pattern = temp_dir / f"frame_%05d{current_image.suffix}"

    # Explanation of the command:
    # -r {frame_rate}: Input frame rate for the image sequence.
    # -i {image_pattern}: The sequence of images.
    # -i {audio_file}: The audio track.
    # -c:v libx264: A very common and compatible video codec.
    # -tune stillimage: Optimizes the video encoding for static images.
    # -c:a aac -b:a 192k: A common and good quality audio codec and bitrate.
    # -pix_fmt yuv420p: Pixel format for broad compatibility (especially with web players).
    # -shortest: Crucial! Ends the video when the shorter input (audio or video) finishes.
    #            This ensures the video duration perfectly matches the audio.
    command = [
        'ffmpeg',
        '-r', str(frame_rate),
        '-i', str(image_pattern),
        '-i', str(audio_file),
        '-c:v', 'libx264',
        '-tune', 'stillimage',
        '-c:a', 'aac',
        '-b:a', '192k',
        '-pix_fmt', 'yuv420p',
        '-shortest',
        str(output_video_path)
    ]

    print(f"  -> Running FFmpeg command...")
    # To see the full FFmpeg output for debugging, remove `stdout` and `stderr` args.
    result = subprocess.run(command, capture_output=True, text=True)

    if result.returncode != 0:
        print(f"  [ERROR] FFmpeg failed for '{audio_file.name}'.")
        print("  -> FFmpeg stderr:")
        print(result.stderr)
    else:
        print(f"  -> Successfully created video: {output_video_path.name}")

    # 5. Clean up temporary directory
    shutil.rmtree(temp_dir)
    print(f"  -> Cleaned up temporary directory.")


def main():
    parser = argparse.ArgumentParser(description="Create audiobook-style videos from audio files and illustrations.")
    parser.add_argument(
        "book_name",
        type=str,
        help="The name of the book directory inside the 'video' folder (e.g., 'Metamorphosis')."
    )
    args = parser.parse_args()

    # Load project config to find the main content directory
    try:
        with open("config.toml", "rb") as f:
            config = tomllib.load(f)
        content_project_dir_str = config.get("content_project_dir")
        if not content_project_dir_str:
            raise ValueError("'content_project_dir' not found in config.")
        content_project_root = Path(content_project_dir_str)
    except (IOError, ValueError) as e:
        print(f"Error loading config.toml: {e}"); return
    
    # Define directory structure
    video_root = content_project_root / "video"
    book_dir = video_root / args.book_name
    audio_dir = book_dir / "audio"
    illustrations_dir = book_dir / "illustrations"
    output_dir = book_dir / "output"

    # Sanity checks
    if not book_dir.is_dir():
        print(f"Error: Book directory not found at '{book_dir}'"); return
    if not audio_dir.is_dir():
        print(f"Error: Audio directory not found at '{audio_dir}'"); return
    if not illustrations_dir.is_dir():
        print(f"Error: Illustrations directory not found at '{illustrations_dir}'"); return
    
    output_dir.mkdir(exist_ok=True)
    
    # Find all media files
    audio_files = sorted([p for p in audio_dir.iterdir() if p.is_file() and p.suffix.lower() in ['.wav', '.mp3', '.flac']])
    illustration_files = sorted([p for p in illustrations_dir.iterdir() if p.is_file() and p.suffix.lower() in ['.png', '.jpg', '.jpeg']])

    if not audio_files:
        print(f"No audio files found in '{audio_dir}'. Nothing to do."); return
    if not illustration_files:
        print(f"No illustrations found in '{illustrations_dir}'. Cannot create videos."); return

    print(f"Found {len(audio_files)} audio file(s) and {len(illustration_files)} illustration(s).")
    print(f"Videos will be saved to: {output_dir}")

    for audio_path in audio_files:
        create_video_from_audio(
            audio_path,
            illustration_files,
            output_dir,
            DEFAULT_FRAME_RATE,
            DEFAULT_IMAGE_DURATION_SECONDS
        )
    
    print("\n--- All videos processed. ---")


if __name__ == "__main__":
    main()