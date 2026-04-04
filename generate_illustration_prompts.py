# generate_illustration_prompts.py
# Reads a UL0 (pure base-language) weave text file and generates image
# prompts for illustration generation using an LLM.
#
# Usage:
#   python generate_illustration_prompts.py <tts_dir> <chapter_name> [options]
#
# Example:
#   python generate_illustration_prompts.py \
#     E:\...\weave_out\grimms\The_Golden_Bird\tts_files \
#     The_Golden_Bird \
#     --count 4 --style "fairy tale watercolor, storybook illustration"
#
# Or with auto-count from manifest:
#   python generate_illustration_prompts.py \
#     E:\...\weave_out\grimms\The_Golden_Bird\tts_files \
#     The_Golden_Bird \
#     --manifest E:\...\weave_out\grimms\_av_manifest.toml

import argparse
import json
import math
import os
import sys
import tomllib
from pathlib import Path

try:
    import keyring
except ImportError:
    keyring = None

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------
DEFAULT_SENTENCES_PER_ILLUSTRATION = 50
DEFAULT_MINIMUM_COUNT = 3
DEFAULT_STYLE = "fairy tale watercolor, storybook illustration, warm lighting"
DEFAULT_MODEL = "gemini-2.5-flash"

SYSTEM_PROMPT = """\
You are an illustration director for a storybook. Given a segment of story text,
generate a single image prompt suitable for an AI image generator (Google Imagen).

Rules:
- Describe a SINGLE scene that captures the key visual moment of the segment.
- Include characters, setting, action, mood, and lighting.
- Do NOT include any text, speech bubbles, or words in the image.
- Do NOT reference panel layouts or multiple scenes.
- Keep the prompt to 2-4 sentences.
- Focus on visual details that would make a compelling illustration.
- Maintain consistency: refer to characters by description (e.g. "the young man
  with brown hair") rather than by name alone.

Respond with ONLY the image prompt text, nothing else."""


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
def load_ul0_text(tts_dir: Path, book_name: str, chapter_name: str) -> str:
    """Find and load the UL0 (pure English) weave text file.
    Falls back to any available UL file if UL0 doesn't exist."""
    pattern = f"{book_name}_{chapter_name}_UL0.txt"
    ul0_path = tts_dir / pattern
    if not ul0_path.exists():
        # Try finding any UL0 file in the directory
        candidates = list(tts_dir.glob("*_UL0.txt"))
        if candidates:
            ul0_path = candidates[0]
        else:
            # Fall back to any UL file — narrative structure is the same across levels
            any_ul = sorted(tts_dir.glob(f"{book_name}_{chapter_name}_UL*.txt"))
            if not any_ul:
                any_ul = sorted(tts_dir.glob("*_UL*.txt"))
            if any_ul:
                ul0_path = any_ul[0]
                print(f"[INFO] UL0 not found, using fallback: {ul0_path.name}")
            else:
                raise FileNotFoundError(
                    f"No UL file found in {tts_dir}. Expected: {pattern}"
                )
    return ul0_path.read_text(encoding="utf-8")


def split_into_paragraphs(text: str) -> list[str]:
    """Split weave text into paragraphs (one per sentence in the weave format)."""
    paragraphs = []
    for block in text.split("\n\n"):
        stripped = block.strip()
        if stripped:
            paragraphs.append(stripped)
    return paragraphs


def compute_illustration_count(
    sentence_count: int,
    sentences_per: int = DEFAULT_SENTENCES_PER_ILLUSTRATION,
    minimum: int = DEFAULT_MINIMUM_COUNT,
) -> int:
    """max(ceil(sentence_count / sentences_per), minimum)"""
    return max(math.ceil(sentence_count / sentences_per), minimum)


def segment_text_for_illustrations(
    paragraphs: list[str], num_illustrations: int
) -> list[str]:
    """Split paragraphs into num_illustrations roughly-equal segments."""
    if num_illustrations <= 0:
        return []
    if num_illustrations >= len(paragraphs):
        return [p for p in paragraphs]

    segment_size = len(paragraphs) / num_illustrations
    segments = []
    for i in range(num_illustrations):
        start = int(i * segment_size)
        end = int((i + 1) * segment_size)
        segment_text = "\n\n".join(paragraphs[start:end])
        segments.append(segment_text)
    return segments


def load_manifest_illustration_config(manifest_path: Path) -> dict:
    """Load [illustrations] section from an _av_manifest.toml if it exists."""
    if not manifest_path.exists():
        return {}
    with open(manifest_path, "rb") as f:
        data = tomllib.load(f)
    return data.get("illustrations", {})


def generate_prompt_with_gemini(
    genai_module, model_name: str, segment_text: str, style_prefix: str, index: int, total: int
) -> str:
    """Call Gemini to generate an illustration prompt for a text segment."""
    user_msg = (
        f"Style direction: {style_prefix}\n\n"
        f"This is segment {index + 1} of {total} from the story. "
        f"Generate an image prompt for this passage:\n\n"
        f"{segment_text}"
    )

    model = genai_module.GenerativeModel(model_name)
    response = model.generate_content(
        [
            {"role": "user", "parts": [{"text": SYSTEM_PROMPT + "\n\n" + user_msg}]},
        ],
    )
    return response.text.strip()


def save_prompts_toml(prompts: list[dict], output_path: Path):
    """Save prompts to a TOML file (human-editable)."""
    lines = [
        "# Auto-generated illustration prompts.",
        "# Edit freely — the image generator reads this file.",
        "",
    ]
    for p in prompts:
        lines.append(f"[[prompt]]")
        lines.append(f'index = {p["index"]}')
        # Multi-line TOML string for the prompt text
        lines.append(f'text = """{p["text"]}"""')
        lines.append(f'style = """{p["style"]}"""')
        # Paragraph range for reference
        lines.append(f'paragraph_start = {p["paragraph_start"]}')
        lines.append(f'paragraph_end = {p["paragraph_end"]}')
        lines.append("")
    output_path.write_text("\n".join(lines), encoding="utf-8")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main():
    parser = argparse.ArgumentParser(
        description="Generate illustration prompts from a UL0 weave text file."
    )
    parser.add_argument("tts_dir", type=Path, help="Path to the tts_files/ directory")
    parser.add_argument("chapter_name", help="Chapter name (e.g. The_Golden_Bird)")
    parser.add_argument(
        "--book-name", default="grimms",
        help="Book name prefix for UL0 filename (default: grimms)"
    )
    parser.add_argument(
        "--count", type=int, default=0,
        help="Number of illustrations (0 = auto-compute from sentence count)"
    )
    parser.add_argument(
        "--sentences-per", type=int, default=DEFAULT_SENTENCES_PER_ILLUSTRATION,
        help=f"Sentences per illustration for auto-count (default: {DEFAULT_SENTENCES_PER_ILLUSTRATION})"
    )
    parser.add_argument(
        "--minimum", type=int, default=DEFAULT_MINIMUM_COUNT,
        help=f"Minimum illustration count (default: {DEFAULT_MINIMUM_COUNT})"
    )
    parser.add_argument(
        "--style", default=DEFAULT_STYLE,
        help="Style prefix prepended to every prompt"
    )
    parser.add_argument(
        "--model", default=DEFAULT_MODEL,
        help=f"Gemini model name (default: {DEFAULT_MODEL})"
    )
    parser.add_argument(
        "--manifest", type=Path, default=None,
        help="Path to _av_manifest.toml (overrides --count, --style, etc.)"
    )
    parser.add_argument(
        "--output", type=Path, default=None,
        help="Output path for _prompts.toml (default: <chapter>/illustrations/_prompts.toml)"
    )
    parser.add_argument("--dry-run", action="store_true", help="Show segments without calling LLM")
    args = parser.parse_args()

    # --- Load API key: try OS keyring first, then GOOGLE_API_KEY env var ---
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

    # --- Load manifest overrides ---
    style = args.style
    sentences_per = args.sentences_per
    minimum = args.minimum
    model_name = args.model
    count = args.count

    if args.manifest:
        cfg = load_manifest_illustration_config(args.manifest)
        if "style_prefix" in cfg:
            style = cfg["style_prefix"]
        if "sentences_per_illustration" in cfg:
            sentences_per = cfg["sentences_per_illustration"]
        if "minimum_count" in cfg:
            minimum = cfg["minimum_count"]
        if "model" in cfg:
            model_name = cfg["model"]

    # --- Load and parse text ---
    print(f"Loading UL0 text from: {args.tts_dir}")
    text = load_ul0_text(args.tts_dir, args.book_name, args.chapter_name)
    paragraphs = split_into_paragraphs(text)

    # First paragraph is usually the title — skip it for illustration purposes
    title = paragraphs[0] if paragraphs else "Untitled"
    story_paragraphs = paragraphs[1:] if len(paragraphs) > 1 else paragraphs
    sentence_count = len(story_paragraphs)

    # --- Compute count ---
    if count <= 0:
        count = compute_illustration_count(sentence_count, sentences_per, minimum)

    print(f"Title: {title}")
    print(f"Paragraphs (sentences): {sentence_count}")
    print(f"Illustrations to generate: {count}")
    print(f"Style: {style}")
    print(f"Model: {model_name}")
    print()

    # --- Segment text ---
    segments = segment_text_for_illustrations(story_paragraphs, count)

    if args.dry_run:
        for i, seg in enumerate(segments):
            preview = seg[:200] + "..." if len(seg) > 200 else seg
            print(f"  [{i + 1}/{count}] ({len(seg)} chars): {preview}")
        print("\nDry run complete. No LLM calls made.")
        return

    # --- Configure Gemini ---
    import google.generativeai as genai
    genai.configure(api_key=api_key)

    # --- Generate prompts ---
    prompts = []
    seg_size = len(story_paragraphs) / count
    for i, segment in enumerate(segments):
        p_start = int(i * seg_size) + 1  # 1-based
        p_end = int((i + 1) * seg_size)
        print(f"  [{i + 1}/{count}] Generating prompt for paragraphs {p_start}-{p_end}...")
        prompt_text = generate_prompt_with_gemini(
            genai, model_name, segment, style, i, count
        )
        print(f"    -> {prompt_text[:100]}...")
        prompts.append({
            "index": i + 1,
            "text": prompt_text,
            "style": style,
            "paragraph_start": p_start,
            "paragraph_end": p_end,
        })

    # --- Save output ---
    if args.output:
        output_path = args.output
    else:
        # Default: sibling of tts_dir → ../illustrations/_prompts.toml
        output_path = args.tts_dir.parent / "illustrations" / "_prompts.toml"
    output_path.parent.mkdir(parents=True, exist_ok=True)
    save_prompts_toml(prompts, output_path)
    print(f"\nSaved {len(prompts)} prompts to: {output_path}")


if __name__ == "__main__":
    main()
