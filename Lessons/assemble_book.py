"""
assemble_book.py — Build one Books/lessons_NNNN-MMMM.txt by concatenating
the matching LLM_OUT/lesson_NNNN.txt bodies with %%META directives.

Usage:
    python Lessons/assemble_book.py <book_number>

book_number is 1-based. Books are dense-packed: book 1 = the first 24
records of lemma_senses_all.jsonl (sorted by rank), book 2 = the next 24,
etc. The 22 missing ranks are skipped during packing, so a book's
filename uses the ACTUAL min/max rank of its 24 records.

The output is idempotent: re-running rebuilds the file from current
LLM_OUT/ contents. Existing %%META directives in LLM_OUT bodies are
forbidden (the prompt says so) but we don't enforce here.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

LESSONS_PER_BOOK = 24
ROOT = Path(__file__).parent
SENSES_PATH = ROOT / "lemma_senses_all.jsonl"
LLM_OUT_DIR = ROOT / "LLM_OUT"
BOOKS_DIR = ROOT / "Books"


def load_records() -> list[dict]:
    recs = []
    for line in SENSES_PATH.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        recs.append(json.loads(line))
    recs.sort(key=lambda r: r["rank"])
    return recs


def book_records(book_number: int, all_recs: list[dict]) -> list[dict]:
    if book_number < 1:
        raise ValueError(f"book_number must be >= 1, got {book_number}")
    start = (book_number - 1) * LESSONS_PER_BOOK
    end = start + LESSONS_PER_BOOK
    if start >= len(all_recs):
        raise ValueError(
            f"book_number {book_number} out of range "
            f"(only {len(all_recs)} records, max book = "
            f"{(len(all_recs) + LESSONS_PER_BOOK - 1) // LESSONS_PER_BOOK})"
        )
    return all_recs[start:end]


def book_filename(recs: list[dict]) -> str:
    rmin = recs[0]["rank"]
    rmax = recs[-1]["rank"]
    return f"lessons_{rmin:04d}-{rmax:04d}.txt"


def lesson_path(rec: dict) -> Path:
    return LLM_OUT_DIR / f"lesson_{rec['rank']:04d}.txt"


def assemble(book_number: int) -> Path:
    all_recs = load_records()
    recs = book_records(book_number, all_recs)

    # Verify all member lessons exist
    missing = [r for r in recs if not lesson_path(r).exists()]
    if missing:
        names = ", ".join(f"rank={r['rank']} ({r['normalized_lemma']})"
                          for r in missing)
        raise FileNotFoundError(f"missing LLM_OUT lessons: {names}")

    BOOKS_DIR.mkdir(parents=True, exist_ok=True)
    out_path = BOOKS_DIR / book_filename(recs)

    parts: list[str] = [
        "%%META source_language: es%%",
        "%%META target_language: es%%",
        "%%META teaching_mode: on%%",
        "%%META source_is_basic: on%%",
        "",
    ]
    for rec in recs:
        body = lesson_path(rec).read_text(encoding="utf-8").strip()
        chapter_name = f"lesson_{rec['rank']:04d}_{rec['normalized_lemma']}"
        parts.append(f"%%META lm_entry: bas={rec['rank']}%%")
        parts.append(f"%%META chapter: {chapter_name}%%")
        parts.append("")
        parts.append(body)
        parts.append("")  # blank line between lessons

    out_path.write_text("\n".join(parts).rstrip() + "\n", encoding="utf-8")
    return out_path


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("book_number", type=int, help="1-based book number")
    args = p.parse_args()

    try:
        out = assemble(args.book_number)
    except (FileNotFoundError, ValueError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1

    print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
