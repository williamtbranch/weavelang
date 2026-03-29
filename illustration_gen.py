# illustration_gen.py
# Reads _prompts.toml and generates illustrations using Google Imagen API.
#
# Usage:
#   python illustration_gen.py <prompts_file> [options]
#
# Example:
#   python illustration_gen.py \
#     E:\...\weave_out\grimms\The_Golden_Bird\illustrations\_prompts.toml
#
#   python illustration_gen.py \
#     E:\...\weave_out\grimms\The_Golden_Bird\illustrations\_prompts.toml \
#     --index 2 --size 1792x1024

import argparse
import os
import sys
import time
import tomllib
from pathlib import Path

from dotenv import load_dotenv

try:
    import keyring
except ImportError:
    keyring = None

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------
DEFAULT_SIZE = "1792x1024"  # Landscape, good for video backgrounds
DEFAULT_IMAGEN_MODEL = "imagen-4.0-generate-001"
MAX_RETRIES = 3
RETRY_DELAY = 10

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
def load_prompts(prompts_path: Path) -> list[dict]:
    """Load prompts from a _prompts.toml file."""
    with open(prompts_path, "rb") as f:
        data = tomllib.load(f)
    return data.get("prompt", [])


def generate_image(client, model: str, prompt_text: str, style: str, size: str) -> bytes:
    """Call Google Imagen API to generate a PNG image."""
    full_prompt = f"{style}. {prompt_text}" if style else prompt_text

    width, height = (int(x) for x in size.split("x"))

    for attempt in range(MAX_RETRIES):
        try:
            response = client.models.generate_images(
                model=model,
                prompt=full_prompt,
                config={
                    "number_of_images": 1,
                    "aspect_ratio": _aspect_ratio_from_size(width, height),
                },
            )
            if response.generated_images:
                return response.generated_images[0].image.image_bytes
            raise RuntimeError("Imagen returned no images")
        except Exception as e:
            if attempt < MAX_RETRIES - 1:
                print(f"    Retry {attempt + 1}/{MAX_RETRIES}: {e}")
                time.sleep(RETRY_DELAY)
            else:
                raise


def _aspect_ratio_from_size(width: int, height: int) -> str:
    """Map pixel dimensions to Imagen aspect ratio string."""
    ratio = width / height
    if abs(ratio - 16 / 9) < 0.1:
        return "16:9"
    if abs(ratio - 3 / 4) < 0.1:
        return "3:4"
    if abs(ratio - 4 / 3) < 0.1:
        return "4:3"
    if abs(ratio - 9 / 16) < 0.1:
        return "9:16"
    # Default landscape
    return "16:9"


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main():
    parser = argparse.ArgumentParser(
        description="Generate illustrations from _prompts.toml using Google Imagen."
    )
    parser.add_argument("prompts_file", type=Path, help="Path to _prompts.toml")
    parser.add_argument(
        "--index", type=int, default=0,
        help="Generate only prompt N (1-based). 0 = all (default)"
    )
    parser.add_argument(
        "--size", default=DEFAULT_SIZE,
        help=f"Image size WxH (default: {DEFAULT_SIZE})"
    )
    parser.add_argument(
        "--model", default=DEFAULT_IMAGEN_MODEL,
        help=f"Imagen model (default: {DEFAULT_IMAGEN_MODEL})"
    )
    parser.add_argument(
        "--output-dir", type=Path, default=None,
        help="Output directory (default: same as prompts_file parent)"
    )
    parser.add_argument(
        "--env", type=Path, default=None,
        help="Path to .env file"
    )
    parser.add_argument("--dry-run", action="store_true", help="Show prompts without generating")
    args = parser.parse_args()

    # --- Load API key: try OS keyring first (matches Rust app), then env/.env ---
    env_path = args.env or Path(__file__).parent / ".env"
    load_dotenv(dotenv_path=env_path)
    api_key = None
    if keyring:
        try:
            api_key = keyring.get_password("google_api_key.weavelang", "google_api_key")
        except Exception:
            pass
    if not api_key:
        api_key = os.getenv("GOOGLE_API_KEY")
    if not api_key and not args.dry_run:
        print("ERROR: Google API key not found. Set via the app (set key google ...) or GOOGLE_API_KEY env var.")
        sys.exit(1)

    # --- Load prompts ---
    if not args.prompts_file.exists():
        print(f"ERROR: Prompts file not found: {args.prompts_file}")
        sys.exit(1)

    prompts = load_prompts(args.prompts_file)
    if not prompts:
        print("ERROR: No prompts found in file.")
        sys.exit(1)

    output_dir = args.output_dir or args.prompts_file.parent
    output_dir.mkdir(parents=True, exist_ok=True)

    # --- Filter by index ---
    if args.index > 0:
        prompts = [p for p in prompts if p.get("index") == args.index]
        if not prompts:
            print(f"ERROR: No prompt with index {args.index}")
            sys.exit(1)

    print(f"Prompts: {len(prompts)}")
    print(f"Model: {args.model}")
    print(f"Size: {args.size}")
    print(f"Output: {output_dir}")
    print()

    if args.dry_run:
        for p in prompts:
            idx = p.get("index", "?")
            text = p.get("text", "")
            style = p.get("style", "")
            print(f"  [{idx}] style: {style[:60]}...")
            print(f"       prompt: {text[:120]}...")
            print()
        print("Dry run complete.")
        return

    # --- Configure Imagen client ---
    from google import genai
    client = genai.Client(api_key=api_key)

    # --- Generate images ---
    for p in prompts:
        idx = p.get("index", 0)
        text = p.get("text", "")
        style = p.get("style", "")
        filename = f"{idx:03d}.png"
        output_path = output_dir / filename

        if output_path.exists():
            print(f"  [{idx}] SKIP (already exists): {filename}")
            continue

        print(f"  [{idx}] Generating: {filename}...")
        image_bytes = generate_image(client, args.model, text, style, args.size)
        output_path.write_bytes(image_bytes)
        print(f"  [{idx}] Saved: {output_path} ({len(image_bytes)} bytes)")

    print(f"\nDone. Generated illustrations in: {output_dir}")


if __name__ == "__main__":
    main()
