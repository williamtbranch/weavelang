"""
generate_book.py — Drive LLM generation of ONE book's worth of lessons,
then assemble it into Books/lessons_NNNN-MMMM.txt.

Strict single-book scope (no batch mode, no --all): producing more than
one book requires invoking this script multiple times by hand. This
guardrail keeps year-long rollout costs predictable.

Usage:
    python Lessons/generate_book.py <book_number>
    python Lessons/generate_book.py 1
    python Lessons/generate_book.py 1 --force

Behavior:
  - Loads Lessons/lemma_senses_all.jsonl, sorts by rank, dense-packs into
    24-lesson books. Book N = records [(N-1)*24 : N*24].
  - Up-to-date guard: refuses to run if Books/lessons_*.txt is newer than
    every dependency (24 LLM_OUT files, generate_lesson.prompt,
    referenced plugin prompts, special_words.txt, lemma_senses_all.jsonl).
    Override with --force.
  - Resume: each rank's existence at LLM_OUT/lesson_{rank:04d}.txt skips
    its LLM call. Delete a file to force regeneration.
  - special_words.txt (TSV: rank<TAB>lemma<TAB>note, '#' comments OK):
    if a rank is listed, the matching plugin prompt at
    Lessons/prompts/lesson_{rank:04d}_{lemma}.prompt is appended to the
    main prompt for that single LLM call. Missing plugin file with a
    special-word entry => fail loud.
  - After all 24 lessons exist, calls assemble_book.assemble() in-process
    to (re)build the book file.
"""

from __future__ import annotations

import argparse
import json
import sys
import threading
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

try:
    import keyring
    from anthropic import Anthropic
except ImportError as exc:
    print(f"ERROR: missing dependency: {exc}", file=sys.stderr)
    sys.exit(1)

sys.path.insert(0, str(Path(__file__).parent))
import assemble_book  # noqa: E402

MODEL = "claude-opus-4-7"
KEYRING_SERVICE = "anthropic_api_key.weavelang"
KEYRING_ACCOUNT = "anthropic_api_key"

LESSONS_PER_BOOK = 24
MAX_TOKENS = 8000  # generous; marquee lemmas can hit ~4300 words
DEFAULT_CONCURRENT = 6

ROOT = Path(__file__).parent
SENSES_PATH = ROOT / "lemma_senses_all.jsonl"
PROMPT_PATH = ROOT / "generate_lesson.prompt"
SPECIAL_WORDS_PATH = ROOT / "special_words.txt"
PLUGIN_DIR = ROOT / "prompts"
LLM_OUT_DIR = ROOT / "LLM_OUT"
BOOKS_DIR = ROOT / "Books"
LOG_PATH = ROOT / "generate_book.log"

_log_lock = threading.Lock()


def log(msg: str) -> None:
    line = msg.rstrip()
    with _log_lock:
        print(line)
        with LOG_PATH.open("a", encoding="utf-8") as fh:
            fh.write(line + "\n")


def get_api_key() -> str:
    key = keyring.get_password(KEYRING_SERVICE, KEYRING_ACCOUNT)
    if not key:
        print("ERROR: no anthropic key in keyring "
              f"(service={KEYRING_SERVICE}, account={KEYRING_ACCOUNT})",
              file=sys.stderr)
        sys.exit(1)
    return key


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
        raise SystemExit(f"book_number must be >= 1, got {book_number}")
    start = (book_number - 1) * LESSONS_PER_BOOK
    end = start + LESSONS_PER_BOOK
    if start >= len(all_recs):
        max_book = (len(all_recs) + LESSONS_PER_BOOK - 1) // LESSONS_PER_BOOK
        raise SystemExit(
            f"book_number {book_number} out of range; max = {max_book}"
        )
    return all_recs[start:end]


def load_special_words() -> dict[int, str]:
    """Return {rank: lemma} for entries listed in special_words.txt."""
    if not SPECIAL_WORDS_PATH.exists():
        return {}
    out: dict[int, str] = {}
    for raw in SPECIAL_WORDS_PATH.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) < 2:
            log(f"WARN: malformed special_words line: {raw!r}")
            continue
        try:
            rank = int(parts[0])
        except ValueError:
            log(f"WARN: non-integer rank in special_words: {raw!r}")
            continue
        out[rank] = parts[1].strip()
    return out


def plugin_prompt_path(rank: int, lemma: str) -> Path:
    return PLUGIN_DIR / f"lesson_{rank:04d}_{lemma}.prompt"


def lesson_out_path(rank: int) -> Path:
    return LLM_OUT_DIR / f"lesson_{rank:04d}.txt"


def book_out_path(recs: list[dict]) -> Path:
    return BOOKS_DIR / f"lessons_{recs[0]['rank']:04d}-{recs[-1]['rank']:04d}.txt"


def is_up_to_date(book_path: Path, deps: list[Path]) -> bool:
    if not book_path.exists():
        return False
    book_mtime = book_path.stat().st_mtime
    for dep in deps:
        if not dep.exists():
            return False
        if dep.stat().st_mtime > book_mtime:
            return False
    return True


def build_prompt(rec: dict, base_prompt: str, plugin_text: str | None) -> str:
    lemma = rec["normalized_lemma"]
    rank = rec["rank"]
    record_json = json.dumps(rec, ensure_ascii=False, indent=2)

    body = base_prompt
    if plugin_text:
        body = body.rstrip() + "\n\n" + plugin_text.strip() + "\n"

    body = body.replace("{{LEMMA}}", lemma)
    body = body.replace("{{RANK}}", f"{rank:04d}")
    body = body.replace("{{LEMMA_RECORD_JSON}}", record_json)
    return body


def call_llm(client: Anthropic, user_msg: str, max_retries: int = 3) -> str:
    import time
    last_exc: Exception | None = None
    for attempt in range(max_retries):
        try:
            msg = client.messages.create(
                model=MODEL,
                max_tokens=MAX_TOKENS,
                messages=[{"role": "user", "content": user_msg}],
            )
            return "".join(getattr(b, "text", "") for b in msg.content).strip()
        except Exception as exc:  # noqa: BLE001
            last_exc = exc
            if attempt < max_retries - 1:
                time.sleep(5.0 * (attempt + 1))
            else:
                raise
    raise last_exc  # type: ignore[misc]


def generate_one(rec: dict, client: Anthropic, base_prompt: str,
                 special_ranks: dict[int, str]) -> tuple[int, bool, str]:
    """Returns (rank, generated_now, status_msg). Skips if file exists."""
    rank = rec["rank"]
    out = lesson_out_path(rank)
    if out.exists():
        return rank, False, f"skip rank={rank} (exists)"

    plugin_text: str | None = None
    if rank in special_ranks:
        path = plugin_prompt_path(rank, special_ranks[rank])
        if not path.exists():
            return rank, False, (
                f"FAIL rank={rank} ({rec['normalized_lemma']}): "
                f"special_words lists rank but plugin prompt missing: {path}"
            )
        plugin_text = path.read_text(encoding="utf-8")

    prompt = build_prompt(rec, base_prompt, plugin_text)
    try:
        text = call_llm(client, prompt)
    except Exception as exc:  # noqa: BLE001
        return rank, False, f"FAIL rank={rank}: {type(exc).__name__}: {exc}"

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(text + ("\n" if not text.endswith("\n") else ""),
                   encoding="utf-8")
    return rank, True, f"ok rank={rank} ({rec['normalized_lemma']}) {len(text)} chars"


def main() -> int:
    p = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("book_number", type=int, help="1-based book number")
    p.add_argument("--force", action="store_true",
                   help="Bypass up-to-date guard")
    p.add_argument("--concurrent", type=int, default=DEFAULT_CONCURRENT,
                   help=f"Worker threads (default {DEFAULT_CONCURRENT})")
    p.add_argument("--dry-run", action="store_true",
                   help="Plan only; no LLM calls, no writes")
    args = p.parse_args()

    all_recs = load_records()
    recs = book_records(args.book_number, all_recs)
    book_path = book_out_path(recs)
    special_ranks = load_special_words()

    deps: list[Path] = [PROMPT_PATH, SENSES_PATH]
    if SPECIAL_WORDS_PATH.exists():
        deps.append(SPECIAL_WORDS_PATH)
    for rec in recs:
        deps.append(lesson_out_path(rec["rank"]))
        if rec["rank"] in special_ranks:
            deps.append(plugin_prompt_path(rec["rank"],
                                           special_ranks[rec["rank"]]))

    log(f"=== book {args.book_number}: ranks "
        f"{recs[0]['rank']}–{recs[-1]['rank']} ({len(recs)} lessons) ===")
    log(f"target: {book_path}")

    if not args.force and is_up_to_date(book_path, deps):
        log(f"up-to-date: {book_path} (use --force to rebuild)")
        return 0

    pending = [r for r in recs if not lesson_out_path(r["rank"]).exists()]
    log(f"already on disk: {len(recs) - len(pending)} / {len(recs)}; "
        f"pending: {len(pending)}")

    if args.dry_run:
        for r in pending:
            tag = " [special]" if r["rank"] in special_ranks else ""
            log(f"  would generate rank={r['rank']} {r['normalized_lemma']}{tag}")
        log("(dry-run; nothing written)")
        return 0

    if pending:
        base_prompt = PROMPT_PATH.read_text(encoding="utf-8")
        client = Anthropic(api_key=get_api_key())

        failures: list[str] = []
        with ThreadPoolExecutor(max_workers=args.concurrent) as pool:
            futs = {
                pool.submit(generate_one, r, client, base_prompt, special_ranks): r
                for r in pending
            }
            for fut in as_completed(futs):
                rank, ok, msg = fut.result()
                log(msg)
                if not ok and msg.startswith("FAIL"):
                    failures.append(msg)

        if failures:
            log(f"{len(failures)} lesson(s) failed; not assembling book")
            return 2

    # All 24 LLM_OUT files should now exist; assemble.
    try:
        out = assemble_book.assemble(args.book_number)
    except (FileNotFoundError, ValueError) as exc:
        log(f"ERROR during assembly: {exc}")
        return 3

    log(f"assembled {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
