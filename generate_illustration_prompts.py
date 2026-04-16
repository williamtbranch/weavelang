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
ILLUSTRATION_MAP_FILENAME = "_illustration_map.json"

SYSTEM_PROMPT = """\
You are an illustration director for a storybook. Given a segment of story text,
generate a single image prompt suitable for an AI image generator.

Rules:
- Describe a SINGLE scene that captures the key visual moment of the segment.
- Include characters, setting, action, mood, and lighting.
- Do NOT include any text, speech bubbles, or words in the image.
- Do NOT reference panel layouts or multiple scenes.
- Keep the prompt to 2-4 sentences.
- Focus on visual details that would make a compelling illustration.
- When characters from the CHARACTER BIBLE appear in the scene, ALWAYS use their
  name AND a brief visual reminder (e.g. "Dorothy, the freckled girl in a blue
  gingham dress"). This ensures the image generator maintains consistency.
- Adapt character descriptions to the scene context — if a character has been
  muddied, disguised, injured, or transformed in the current passage, mention
  that modification explicitly.
- Only include characters who are PRESENT in the segment. Do not inject
  characters who are absent from this part of the story.

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


def load_characters(characters_path: Path) -> list[dict]:
    """Load character bible from a _characters.toml file."""
    if not characters_path.exists():
        return []
    with open(characters_path, "rb") as f:
        data = tomllib.load(f)
    return data.get("character", [])


def format_character_bible(characters: list[dict]) -> str:
    """Format the character list into a text block for the LLM prompt."""
    if not characters:
        return ""
    lines = ["CHARACTER BIBLE (use these descriptions for visual consistency):"]
    for ch in characters:
        name = ch.get("name", "Unknown")
        desc = ch.get("description", "")
        aliases = ch.get("aliases", [])
        alias_str = f' (also referred to as: {", ".join(aliases)})' if aliases else ""
        lines.append(f"- {name}{alias_str}: {desc}")
    return "\n".join(lines)


def build_scene_context(paragraphs: list[str], seg_start: int, seg_end: int,
                        context_radius: int = 25) -> str:
    """Return ~50 sentences of surrounding context (25 before + 25 after the segment)."""
    before_start = max(0, seg_start - context_radius)
    after_end = min(len(paragraphs), seg_end + context_radius)
    before = paragraphs[before_start:seg_start]
    after = paragraphs[seg_end:after_end]
    parts = []
    if before:
        parts.append("[PRECEDING CONTEXT]\n" + "\n".join(before))
    if after:
        parts.append("[FOLLOWING CONTEXT]\n" + "\n".join(after))
    return "\n\n".join(parts)


def generate_prompt_with_gemini(
    genai_module, model_name: str, segment_text: str, style_prefix: str,
    index: int, total: int, book_title: str = "",
    character_bible: str = "", scene_context: str = ""
) -> str:
    """Call Gemini to generate an illustration prompt for a text segment."""
    book_context = f' from "{book_title}"' if book_title else ""

    parts = [f"Style direction: {style_prefix}\n"]
    if character_bible:
        parts.append(character_bible + "\n")
    parts.append(
        f"This is segment {index + 1} of {total}{book_context}.\n"
        f"Generate an image prompt for this passage:\n\n"
        f"{segment_text}"
    )
    if scene_context:
        parts.append(
            "\n[SURROUNDING STORY CONTEXT — use this to understand the scene "
            "but generate a prompt ONLY for the passage above]\n" + scene_context
        )

    user_msg = "\n".join(parts)

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
    parser.add_argument(
        "--characters", type=Path, default=None,
        help="Path to _characters.toml (default: auto-detect in illustrations dir)"
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

    # --- Load character bible ---
    characters_path = args.characters
    if not characters_path:
        # Auto-detect: look in the illustrations dir
        illustrations_dir = args.tts_dir.parent / "illustrations"
        characters_path = illustrations_dir / "_characters.toml"
    characters = load_characters(characters_path)
    character_bible = format_character_bible(characters)
    if characters:
        print(f"Character bible: {len(characters)} characters loaded from {characters_path}")
    else:
        print("No character bible found — prompts will use generic descriptions.")
    print()

    # --- Segment text ---
    segments = segment_text_for_illustrations(story_paragraphs, count)

    if args.dry_run:
        for i, seg in enumerate(segments):
            preview = seg[:200] + "..." if len(seg) > 200 else seg
            print(f"  [{i + 1}/{count}] ({len(seg)} chars): {preview}")
        if character_bible:
            print(f"\nCharacter bible:\n{character_bible}")
        print("\nDry run complete. No LLM calls made.")
        return

    # --- Configure Gemini ---
    import google.generativeai as genai
    genai.configure(api_key=api_key)

    # --- Generate prompts ---
    prompts = []
    seg_size = len(story_paragraphs) / count
    for i, segment in enumerate(segments):
        p_start = int(i * seg_size)      # 0-based index into story_paragraphs
        p_end = int((i + 1) * seg_size)
        # Derive a human-readable book title from the chapter name
        book_title = args.chapter_name.replace("_", " ")
        # Build surrounding scene context (~50 sentences)
        scene_context = build_scene_context(story_paragraphs, p_start, p_end)
        print(f"  [{i + 1}/{count}] Generating prompt for paragraphs {p_start + 1}-{p_end}...")
        prompt_text = generate_prompt_with_gemini(
            genai, model_name, segment, style, i, count, book_title,
            character_bible=character_bible, scene_context=scene_context,
        )
        print(f"    -> {prompt_text[:100]}...")
        prompts.append({
            "index": i + 1,
            "text": prompt_text,
            "style": style,
            "paragraph_start": p_start + 1,  # 1-based for display
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

    # --- Save _illustration_map.json for video synchronization ---
    illustration_map = {
        "illustrations": [
            {
                "index": p["index"],
                "file": f"{p['index']:03d}.png",
                "start_sentence": p["paragraph_start"],
                "end_sentence": p["paragraph_end"],
            }
            for p in prompts
        ]
    }
    map_path = output_path.parent / ILLUSTRATION_MAP_FILENAME
    import json
    with open(map_path, "w") as f:
        json.dump(illustration_map, f, indent=4)
    print(f"Saved illustration map to: {map_path}")


if __name__ == "__main__":
    main()
