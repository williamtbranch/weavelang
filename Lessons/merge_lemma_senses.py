#!/usr/bin/env python3
"""Merge Flashcards/lemma_senses_*.jsonl into Lessons/lemma_senses_all.jsonl.

One output row per lemma rank, grouping all senses under that rank.

Output schema (one JSON object per line):

    {
      "rank": <int>,
      "normalized_lemma": <str>,
      "senses": [
        {
          "sense_index_in_lemma": <int>,
          "unnormalized": <str>,
          "pos": <str>,
          "gloss": <str>,
          "spanish": <str>,
          "english": <str>
        },
        ...
      ]
    }

Records are emitted in ascending `rank` order; senses within a record
are sorted by `sense_index_in_lemma`. Re-running is idempotent (output
file is overwritten).
"""

from __future__ import annotations

import json
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SOURCE_FILES = [
    REPO_ROOT / "Flashcards" / "lemma_senses_1-20.jsonl",
    REPO_ROOT / "Flashcards" / "lemma_senses_21-100.jsonl",
    REPO_ROOT / "Flashcards" / "lemma_senses_101-500.jsonl",
    REPO_ROOT / "Flashcards" / "lemma_senses_501-2000.jsonl",
]
OUTPUT_FILE = REPO_ROOT / "Lessons" / "lemma_senses_all.jsonl"

SENSE_KEYS = ("sense_index_in_lemma", "unnormalized", "pos", "gloss",
              "spanish", "english")


def main() -> None:
    by_rank: dict[int, dict] = {}

    for src in SOURCE_FILES:
        if not src.exists():
            raise SystemExit(f"missing input: {src}")
        with src.open("r", encoding="utf-8") as fh:
            for line_no, line in enumerate(fh, 1):
                line = line.strip()
                if not line:
                    continue
                row = json.loads(line)
                rank = int(row["rank"])
                lemma = row["normalized_lemma"]
                bucket = by_rank.setdefault(rank, {
                    "rank": rank,
                    "normalized_lemma": lemma,
                    "senses": [],
                })
                if bucket["normalized_lemma"] != lemma:
                    raise SystemExit(
                        f"{src}:{line_no}: rank {rank} has conflicting "
                        f"lemmas {bucket['normalized_lemma']!r} vs {lemma!r}"
                    )
                bucket["senses"].append({k: row.get(k) for k in SENSE_KEYS})

    # Stable, deterministic ordering.
    ordered_ranks = sorted(by_rank)
    OUTPUT_FILE.parent.mkdir(parents=True, exist_ok=True)
    with OUTPUT_FILE.open("w", encoding="utf-8") as out:
        for rank in ordered_ranks:
            rec = by_rank[rank]
            rec["senses"].sort(key=lambda s: (s["sense_index_in_lemma"] is None,
                                              s["sense_index_in_lemma"]))
            out.write(json.dumps(rec, ensure_ascii=False) + "\n")

    total_senses = sum(len(r["senses"]) for r in by_rank.values())
    print(f"wrote {len(ordered_ranks)} lemma records "
          f"({total_senses} senses) → {OUTPUT_FILE}")
    if ordered_ranks:
        lo, hi = ordered_ranks[0], ordered_ranks[-1]
        print(f"rank range: {lo}..{hi}")


if __name__ == "__main__":
    main()
