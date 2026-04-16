# extract_characters.py
# Reads a UL0 (pure base-language) weave text file and extracts a character
# bible using an LLM.  Outputs _characters.toml for use by the illustration
# pipeline (prompt enrichment + reference-image generation).
#
# Usage:
#   python extract_characters.py <tts_dir> <chapter_name> [options]
#
# Example:
#   python extract_characters.py \
#     E:\...\weave_out\grimms\The_Golden_Bird\tts_files \
#     The_Golden_Bird

import argparse
import json
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
DEFAULT_MODEL = "gemini-2.5-flash"

SYSTEM_PROMPT = """\
You are a literary analyst extracting a character bible for illustration purposes.
Given the full text of a story, identify ALL named characters and significant
unnamed recurring characters (e.g. "the old woman", "the king's horse").

For each character provide:
- name: The character's primary name or identifier.
- aliases: A list of other ways the text refers to this character
  (e.g. ["the girl", "the child", "she"] for Dorothy).
- description: A detailed VISUAL description: physical appearance, approximate
  age, build, hair colour/style, eye colour, skin tone, clothing, accessories,
  and any distinguishing features.  Be specific — these descriptions will drive
  AI image generation.  2-4 sentences.
- role: One of "protagonist", "deuteragonist", "antagonist", "supporting",
  "animal_companion", "minor".

Output valid JSON and nothing else:
{
  "characters": [
    {
      "name": "Dorothy",
      "aliases": ["the girl", "the child"],
      "description": "A young girl of about 10 years old with brown hair in two braids, freckled cheeks, and bright blue eyes. She wears a blue and white gingham dress with a white pinafore apron and simple black shoes.",
      "role": "protagonist"
    }
  ]
}

Rules:
- Include ONLY characters who appear in multiple scenes or are important to the plot.
- Be very specific about visual details — colours, textures, shapes.
- If the text explicitly describes a character's appearance, use those details exactly.
- If the text does not describe a character, infer reasonable visual details that
  are consistent with the story's setting, era, and cultural context.
- Do NOT include generic crowds, unnamed one-off bystanders, or abstract entities.
- Respond with ONLY the JSON. No markdown fences, no commentary."""


# ---------------------------------------------------------------------------
# Helpers (shared with generate_illustration_prompts.py)
# ---------------------------------------------------------------------------
def load_ul0_text(tts_dir: Path, book_name: str, chapter_name: str) -> str:
    """Find and load the UL0 (pure English) weave text file."""
    pattern = f"{book_name}_{chapter_name}_UL0.txt"
    ul0_path = tts_dir / pattern
    if not ul0_path.exists():
        candidates = list(tts_dir.glob("*_UL0.txt"))
        if candidates:
            ul0_path = candidates[0]
        else:
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


def save_characters_toml(characters: list[dict], output_path: Path):
    """Write characters to a human-editable TOML file."""
    lines = [
        "# Auto-extracted character bible for illustration consistency.",
        "# Edit freely — the illustration pipeline reads this file.",
        "# Re-run 'av generate characters' to regenerate (overwrites edits).",
        "",
    ]
    for ch in characters:
        lines.append("[[character]]")
        lines.append(f'name = "{_escape_toml(ch["name"])}"')
        # aliases as inline array
        alias_strs = ", ".join(f'"{_escape_toml(a)}"' for a in ch.get("aliases", []))
        lines.append(f"aliases = [{alias_strs}]")
        lines.append(f'description = """{ch["description"]}"""')
        lines.append(f'role = "{ch.get("role", "supporting")}"')
        lines.append("")
    output_path.write_text("\n".join(lines), encoding="utf-8")


def _escape_toml(s: str) -> str:
    """Escape characters problematic in TOML strings."""
    return s.replace("\\", "\\\\").replace('"', '\\"')


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main():
    parser = argparse.ArgumentParser(
        description="Extract a character bible from UL0 text using an LLM."
    )
    parser.add_argument("tts_dir", type=Path, help="Path to the tts_files/ directory")
    parser.add_argument("chapter_name", help="Chapter name (e.g. The_Golden_Bird)")
    parser.add_argument(
        "--book-name", default="grimms",
        help="Book name prefix for UL0 filename (default: grimms)"
    )
    parser.add_argument(
        "--model", default=DEFAULT_MODEL,
        help=f"Gemini model name (default: {DEFAULT_MODEL})"
    )
    parser.add_argument(
        "--output", type=Path, default=None,
        help="Output path for _characters.toml (default: <chapter>/illustrations/_characters.toml)"
    )
    parser.add_argument("--dry-run", action="store_true", help="Show text stats without calling LLM")
    args = parser.parse_args()

    # --- API key ---
    api_key = None
    if keyring:
        try:
            api_key = keyring.get_password("google_api_key.weavelang", "google_api_key")
        except Exception:
            pass
    if not api_key:
        api_key = os.getenv("GOOGLE_API_KEY")
    if not api_key and not args.dry_run:
        print("ERROR: Google API key not found.")
        sys.exit(1)

    # --- Load text ---
    print(f"Loading UL0 text from: {args.tts_dir}")
    text = load_ul0_text(args.tts_dir, args.book_name, args.chapter_name)
    print(f"Text length: {len(text)} chars")

    if args.dry_run:
        print(f"Would send {len(text)} chars to {args.model} for character extraction.")
        print("Dry run complete.")
        return

    # --- Call Gemini ---
    from google import genai
    client = genai.Client(api_key=api_key)

    book_title = args.chapter_name.replace("_", " ")
    user_msg = (
        f'The following is the full text of "{book_title}".\n'
        f"Extract all significant characters with detailed visual descriptions.\n\n"
        f"{text}"
    )

    print(f"Calling {args.model} for character extraction...")
    response = client.models.generate_content(
        model=args.model,
        contents=[
            {"role": "user", "parts": [{"text": SYSTEM_PROMPT + "\n\n" + user_msg}]},
        ],
    )

    raw = response.text.strip()
    # Strip markdown fences if present
    if raw.startswith("```"):
        raw = raw.split("\n", 1)[1] if "\n" in raw else raw[3:]
    if raw.endswith("```"):
        raw = raw[: raw.rfind("```")]
    raw = raw.strip()

    try:
        data = json.loads(raw)
    except json.JSONDecodeError as e:
        print(f"ERROR: LLM returned invalid JSON: {e}")
        print(f"Raw response:\n{raw[:500]}")
        sys.exit(1)

    characters = data.get("characters", [])
    if not characters:
        print("WARNING: LLM returned no characters.")
        sys.exit(1)

    print(f"Extracted {len(characters)} characters:")
    for ch in characters:
        print(f"  - {ch['name']} ({ch.get('role', '?')}): {ch['description'][:80]}...")

    # --- Save output ---
    if args.output:
        output_path = args.output
    else:
        output_path = args.tts_dir.parent / "illustrations" / "_characters.toml"
    output_path.parent.mkdir(parents=True, exist_ok=True)
    save_characters_toml(characters, output_path)
    print(f"\nSaved character bible to: {output_path}")


if __name__ == "__main__":
    main()
