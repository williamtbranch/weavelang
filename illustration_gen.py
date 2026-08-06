# illustration_gen.py
# Reads _prompts.toml and generates illustrations using the Gemini native
# image generation API (gemini-3.1-flash-image-preview or similar).
# Supports character reference images for visual consistency.
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
#     --size 2K --aspect-ratio 16:9

import argparse
import io
import os
import sys
import time
import tomllib
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

try:
    import keyring
except ImportError:
    keyring = None

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------
DEFAULT_ASPECT_RATIO = "16:9"
DEFAULT_IMAGE_SIZE = "2K"
DEFAULT_MODEL = "gemini-3.1-flash-image-preview"
MAX_RETRIES = 3
RETRY_DELAY = 10
DEFAULT_CONCURRENT_REQUESTS = 1
CHAR_REFS_DIR_NAME = "_char_refs"
# YouTube rejects thumbnails over 2 MB, so JPEG quality is stepped down until
# the encoded image fits rather than failing at upload time.
MAX_THUMBNAIL_BYTES = 2_000_000

# ---------------------------------------------------------------------------
# Character reference helpers
# ---------------------------------------------------------------------------
def load_characters(characters_path: Path) -> list[dict]:
    """Load character bible from a _characters.toml file."""
    if not characters_path.exists():
        return []
    with open(characters_path, "rb") as f:
        data = tomllib.load(f)
    return data.get("character", [])


def generate_character_ref(client, model: str, character: dict,
                           style: str, aspect_ratio: str,
                           image_size: str) -> bytes:
    """Generate a reference portrait for one character using Gemini."""
    name = character["name"]
    desc = character["description"]

    prompt = (
        f"A full-body character reference portrait of {name}, {desc} "
        f"Standing in a neutral pose against a plain light background. "
        f"Portrait orientation (3:4 aspect ratio). "
        f"Style: {style}. "
        f"Clear, well-lit, showing full outfit and distinguishing features. "
        f"No text, no other characters."
    )

    for attempt in range(MAX_RETRIES):
        try:
            response = client.models.generate_content(
                model=model,
                contents=[prompt],
            )
            for candidate in response.candidates:
                for part in candidate.content.parts:
                    if part.inline_data is not None:
                        raw = part.inline_data.data
                        return _ensure_png(raw)
            raise RuntimeError("Gemini returned no image for character reference")
        except Exception as e:
            if attempt < MAX_RETRIES - 1:
                print(f"    Retry {attempt + 1}/{MAX_RETRIES}: {e}")
                time.sleep(RETRY_DELAY)
            else:
                raise


def ensure_character_refs(client, model: str, characters: list[dict],
                          refs_dir: Path, style: str, aspect_ratio: str,
                          image_size: str):
    """Generate missing character reference images."""
    refs_dir.mkdir(parents=True, exist_ok=True)
    for ch in characters:
        name = ch["name"]
        safe_name = name.lower().replace(" ", "_").replace("'", "")
        ref_path = refs_dir / f"{safe_name}.png"
        if ref_path.exists():
            print(f"  [ref] SKIP {name} (already exists)")
            continue
        print(f"  [ref] Generating reference portrait for {name}...")
        img_bytes = generate_character_ref(
            client, model, ch, style, aspect_ratio, image_size
        )
        ref_path.write_bytes(img_bytes)
        print(f"  [ref] Saved: {ref_path} ({len(img_bytes)} bytes)")


def load_ref_images(characters: list[dict], refs_dir: Path) -> dict:
    """Load character reference images from disk. Returns {name: PIL.Image}."""
    from PIL import Image
    ref_images = {}
    for ch in characters:
        name = ch["name"]
        safe_name = name.lower().replace(" ", "_").replace("'", "")
        ref_path = refs_dir / f"{safe_name}.png"
        if ref_path.exists():
            img = Image.open(ref_path)
            img.load()  # Fully decode now so the image is safe to share across threads
            ref_images[name] = img
    return ref_images


def find_characters_in_prompt(prompt_text: str, characters: list[dict]) -> list[str]:
    """Determine which characters are mentioned in a prompt.
    Checks character names and aliases against the prompt text."""
    text_lower = prompt_text.lower()
    found = []
    for ch in characters:
        name = ch["name"]
        # Check name
        if name.lower() in text_lower:
            found.append(name)
            continue
        # Check aliases
        for alias in ch.get("aliases", []):
            if alias.lower() in text_lower:
                found.append(name)
                break
    return found


# ---------------------------------------------------------------------------
# Image generation
# ---------------------------------------------------------------------------
def generate_image(client, model: str, prompt_text: str, style: str,
                   aspect_ratio: str, image_size: str,
                   ref_images: list = None) -> bytes:
    """Call Gemini image generation API. Optionally pass character reference images."""
    full_prompt = f"{style}. {prompt_text}" if style else prompt_text
    # Include aspect ratio in the prompt text as a generation hint
    full_prompt = f"{full_prompt} Image should be {aspect_ratio} aspect ratio, landscape orientation."

    # Build contents: prompt text + any reference images
    contents = [full_prompt]
    if ref_images:
        for img in ref_images:
            contents.append(img)

    for attempt in range(MAX_RETRIES):
        try:
            response = client.models.generate_content(
                model=model,
                contents=contents,
            )
            for candidate in response.candidates:
                for part in candidate.content.parts:
                    if part.inline_data is not None:
                        raw = part.inline_data.data
                        return _ensure_png(raw)
            raise RuntimeError("Gemini returned no image")
        except Exception as e:
            if attempt < MAX_RETRIES - 1:
                print(f"    Retry {attempt + 1}/{MAX_RETRIES}: {e}")
                time.sleep(RETRY_DELAY)
            else:
                raise


def _ensure_png(image_bytes: bytes) -> bytes:
    """Convert image bytes to PNG if they are not already PNG."""
    if image_bytes[:8] == b'\x89PNG\r\n\x1a\n':
        return image_bytes
    from PIL import Image
    img = Image.open(io.BytesIO(image_bytes))
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return buf.getvalue()


def _parse_size(spec: str) -> tuple[int, int] | None:
    """Parse a 'WIDTHxHEIGHT' resize spec. Returns None if absent or malformed."""
    if not spec:
        return None
    try:
        w, h = spec.lower().split("x", 1)
        w, h = int(w), int(h)
    except (ValueError, AttributeError):
        print(f"    Warning: ignoring unparseable resize '{spec}'")
        return None
    return (w, h) if w > 0 and h > 0 else None


def _finalise(image_bytes: bytes, resize: str, suffix: str) -> bytes:
    """Apply the per-prompt resize and container format.

    Thumbnails need exact pixel dimensions and a size ceiling; ordinary scene
    illustrations are left exactly as the model produced them.
    """
    target = _parse_size(resize)
    want_jpeg = suffix.lower() in (".jpg", ".jpeg")
    if target is None and not want_jpeg:
        return _ensure_png(image_bytes)

    from PIL import Image
    img = Image.open(io.BytesIO(image_bytes))

    if target is not None:
        tw, th = target
        # Cover-crop to the target aspect before scaling, so the image is never
        # squashed. Gemini is asked for 16:9 already, so this usually trims a
        # few pixels at most.
        src_ratio = img.width / img.height
        dst_ratio = tw / th
        if abs(src_ratio - dst_ratio) > 0.001:
            if src_ratio > dst_ratio:
                new_w = int(round(img.height * dst_ratio))
                left = (img.width - new_w) // 2
                img = img.crop((left, 0, left + new_w, img.height))
            else:
                new_h = int(round(img.width / dst_ratio))
                top = (img.height - new_h) // 2
                img = img.crop((0, top, img.width, top + new_h))
        img = img.resize((tw, th), Image.LANCZOS)

    buf = io.BytesIO()
    if want_jpeg:
        img = img.convert("RGB")
        for quality in (92, 85, 78, 70, 60):
            buf = io.BytesIO()
            img.save(buf, format="JPEG", quality=quality, optimize=True)
            if buf.tell() <= MAX_THUMBNAIL_BYTES:
                break
    else:
        img.save(buf, format="PNG")
    return buf.getvalue()


def load_prompts(prompts_path: Path) -> list[dict]:
    """Load prompts from a _prompts.toml file."""
    with open(prompts_path, "rb") as f:
        data = tomllib.load(f)
    return data.get("prompt", [])


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main():
    parser = argparse.ArgumentParser(
        description="Generate illustrations from _prompts.toml using Gemini image generation."
    )
    parser.add_argument("prompts_file", type=Path, help="Path to _prompts.toml")
    parser.add_argument(
        "--index", type=int, default=0,
        help="Generate only prompt N (1-based). 0 = all (default)"
    )
    parser.add_argument(
        "--aspect-ratio", default=DEFAULT_ASPECT_RATIO,
        help=f"Image aspect ratio (default: {DEFAULT_ASPECT_RATIO})"
    )
    parser.add_argument(
        "--size", default=DEFAULT_IMAGE_SIZE,
        help=f"Image size: 512, 1K, 2K, or 4K (default: {DEFAULT_IMAGE_SIZE})"
    )
    parser.add_argument(
        "--model", default=DEFAULT_MODEL,
        help=f"Gemini image model (default: {DEFAULT_MODEL})"
    )
    parser.add_argument(
        "--concurrent-requests", type=int, default=DEFAULT_CONCURRENT_REQUESTS,
        help=f"Number of images to generate in parallel (default: {DEFAULT_CONCURRENT_REQUESTS})"
    )
    parser.add_argument(
        "--output-dir", type=Path, default=None,
        help="Output directory (default: same as prompts_file parent)"
    )
    parser.add_argument(
        "--characters", type=Path, default=None,
        help="Path to _characters.toml for reference images (default: auto-detect)"
    )
    parser.add_argument(
        "--no-refs", action="store_true",
        help="Skip character reference images even if _characters.toml exists"
    )
    parser.add_argument("--dry-run", action="store_true", help="Show prompts without generating")
    args = parser.parse_args()

    # --- Load API key ---
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

    # --- Load character bible ---
    characters_path = args.characters
    if not characters_path:
        characters_path = args.prompts_file.parent / "_characters.toml"
    characters = load_characters(characters_path) if not args.no_refs else []

    # --- Filter by index ---
    if args.index > 0:
        prompts = [p for p in prompts if p.get("index") == args.index]
        if not prompts:
            print(f"ERROR: No prompt with index {args.index}")
            sys.exit(1)

    print(f"Prompts: {len(prompts)}")
    print(f"Model: {args.model}")
    print(f"Aspect ratio: {args.aspect_ratio}")
    print(f"Image size: {args.size}")
    print(f"Characters: {len(characters)} loaded")
    print(f"Concurrent requests: {args.concurrent_requests}")
    print(f"Output: {output_dir}")
    print()

    if args.dry_run:
        for p in prompts:
            idx = p.get("index", "?")
            text = p.get("text", "")
            style = p.get("style", "")
            filename = p.get("file") or (f"{idx:03d}.png" if isinstance(idx, int) else "?.png")
            chars_in = find_characters_in_prompt(text, characters)
            print(f"  [{idx}] {filename}  style: {style[:60]}...")
            print(f"       prompt: {text[:120]}...")
            if p.get("resize"):
                print(f"       resize: {p['resize']}")
            if p.get("ref_files"):
                print(f"       refs:   {', '.join(p['ref_files'])}")
            if chars_in:
                print(f"       characters: {', '.join(chars_in)}")
            print()
        print("Dry run complete.")
        return

    # --- Configure Gemini client ---
    from google import genai
    client = genai.Client(api_key=api_key)

    # --- Generate character reference images if needed ---
    refs_dir = output_dir / CHAR_REFS_DIR_NAME
    ref_images_map = {}
    if characters:
        # Get style from first prompt
        first_style = prompts[0].get("style", "") if prompts else ""
        ensure_character_refs(
            client, args.model, characters, refs_dir, first_style,
            args.aspect_ratio, args.size,
        )
        ref_images_map = load_ref_images(characters, refs_dir)
        print(f"  Loaded {len(ref_images_map)} character reference images.\n")

    # --- Generate scene illustrations ---
    def generate_one(p: dict):
        idx = p.get("index", 0)
        text = p.get("text", "")
        style = p.get("style", "")
        filename = p.get("file") or f"{idx:03d}.png"
        output_path = output_dir / filename
        output_path.parent.mkdir(parents=True, exist_ok=True)

        if output_path.exists():
            print(f"  [{idx}] SKIP (already exists): {filename}")
            return

        # Explicit references come first: the diglot thumbnail is generated
        # from the plain thumbnail so it is the same picture with a badge
        # added, not a second interpretation of the same brief.
        from PIL import Image
        scene_refs = []
        for rel in p.get("ref_files", []):
            ref_path = output_dir / rel
            if not ref_path.exists():
                print(f"  [{idx}] Warning: reference image not found: {rel}")
                continue
            ref = Image.open(ref_path)
            ref.load()
            scene_refs.append(ref)

        # Then character refs, up to the Gemini limit of 4 images.
        chars_in = find_characters_in_prompt(text, characters)
        scene_refs += [ref_images_map[name] for name in chars_in if name in ref_images_map]
        scene_refs = scene_refs[:4]

        ref_info = f" + {len(scene_refs)} ref(s)" if scene_refs else ""
        print(f"  [{idx}] Generating: {filename}{ref_info}...")
        image_bytes = generate_image(
            client, args.model, text, style,
            args.aspect_ratio, args.size,
            ref_images=scene_refs if scene_refs else None,
        )
        image_bytes = _finalise(image_bytes, p.get("resize", ""), output_path.suffix)
        output_path.write_bytes(image_bytes)
        print(f"  [{idx}] Saved: {output_path} ({len(image_bytes)} bytes)")

    # Prompts that reference an image produced by another prompt must run
    # afterwards, and in order.
    independent = [p for p in prompts if not p.get("ref_files")]
    dependent = [p for p in prompts if p.get("ref_files")]

    concurrency = max(1, args.concurrent_requests)
    if concurrency == 1:
        for p in independent:
            generate_one(p)
    else:
        with ThreadPoolExecutor(max_workers=concurrency) as executor:
            futures = {executor.submit(generate_one, p): p for p in independent}
            for future in as_completed(futures):
                # Re-raise any exception from a worker so the process exits non-zero
                future.result()

    for p in dependent:
        generate_one(p)

    print(f"\nDone. Generated illustrations in: {output_dir}")


if __name__ == "__main__":
    main()
