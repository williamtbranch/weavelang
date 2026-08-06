"""Interlinear closed-caption (CC) subtitle builder for WeaveLang videos.

Consumes:
  - a CC map JSON exported by the Rust app (``<stem>_cc.json`` in tts_files/):
    per sentence, ordered word/phrase tokens of the spoken (target) language
    with base-language glosses from the inverse diglot mapping.
  - the final TTS audio file (for word-level timing via faster-whisper
    forced-style alignment against the known text).

Produces an ``.ass`` subtitle file that ffmpeg burns into the video:
  - interlinear layout: spoken word above, gloss below, vertically aligned
    (the wider of the two sets the column width)
  - three interlinear rows visible at the bottom of the frame; the row being
    spoken sits in the middle; the top row is fading out; the bottom row
    previews what is ahead
  - the currently spoken word pair is highlighted in yellow
  - all text is white with a black outline so it reads over any illustration

Can also be run standalone:
    python cc_subtitles.py --audio-file X.wav --cc-map X_cc.json \
        --output-dir out/ [--width 1280 --height 720] [--model small]
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import unicodedata
from dataclasses import dataclass, field
from difflib import SequenceMatcher
from pathlib import Path

# ---------------------------------------------------------------------------
# Layout constants (design resolution 1280x720; scaled by actual video size)
# ---------------------------------------------------------------------------
DESIGN_H = 720
SP_FONT_SIZE = 35       # spoken-language (Spanish) font size at 720p (+18% for phone legibility)
EN_FONT_SIZE = 26       # gloss (English) font size at 720p (+18% for phone legibility)
CELL_GAP = 18           # horizontal gap between word cells
LINE_GAP = 3            # gap between spoken line and gloss line inside a row
ROW_GAP = 10            # vertical gap between interlinear rows
SIDE_MARGIN = 46        # left/right margin
BOTTOM_MARGIN = 14      # margin below the lowest row
BAND_TOP_PAD = 12       # frosted-glass padding above the top caption row
FADE_MS = 140           # fade in/out on row transitions

FONT_NAME = "Arial"

# Placeholder shown when the diglot map has no suitable base-language
# substitution for a spoken word (the CC map carries a "No_Sub" gloss).
NO_SUB_GLYPH = "~"
_NO_SUB_TOKEN = "no_sub"

# ASS colors are &HAABBGGRR
COL_WHITE = "&H00FFFFFF"
COL_GLOSS = "&H00E8E8E8"
COL_YELLOW = "&H0000FFFF"   # RGB(255,255,0)
COL_OUTLINE = "&H00000000"
ALPHA_TOP = "&HA0&"      # top (fading-away) row: mostly transparent
ALPHA_BOTTOM = "&H50&"   # bottom (upcoming) row: slightly dimmed


# ---------------------------------------------------------------------------
# Data model
# ---------------------------------------------------------------------------
@dataclass
class Cell:
    """One interlinear column: a spoken word/phrase and its gloss."""
    sp: str                 # spoken text (punctuation may be attached)
    en: str                 # gloss text ("" if none)
    sent_n: int             # 1-based sentence number
    ref_lo: int = -1        # first index into the flat reference word list
    ref_hi: int = -1        # last index (inclusive)
    start: float = 0.0      # seconds
    end: float = 0.0
    width: float = 0.0      # layout width (px)
    x: float = 0.0          # center x of the cell within its row


@dataclass
class Row:
    cells: list[Cell] = field(default_factory=list)

    @property
    def start(self) -> float:
        return self.cells[0].start

    @property
    def end(self) -> float:
        return self.cells[-1].end


# ---------------------------------------------------------------------------
# CC map parsing → cells
# ---------------------------------------------------------------------------
def load_cc_map(path: Path) -> dict:
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    if data.get("format") != 1:
        raise ValueError(f"Unsupported CC map format: {data.get('format')}")
    return data


def build_cells(cc_map: dict, sentence_range: tuple[int, int] | None = None) -> list[Cell]:
    """Convert CC map tokens into interlinear cells.

    Punctuation tokens attach to the previous cell's spoken text (or are
    prefixed to the next cell when they open a sentence, e.g. quotes).
    """
    cells: list[Cell] = []
    for sent in cc_map.get("sentences", []):
        n = sent["n"]
        if sentence_range and not (sentence_range[0] <= n <= sentence_range[1]):
            continue
        pending_prefix = ""
        sent_cells: list[Cell] = []
        for tok in sent.get("tokens", []):
            if "w" in tok:
                sp = (pending_prefix + tok["w"]).strip()
                pending_prefix = ""
                if not sp:
                    continue
                gloss = tok.get("g", "").strip()
                # The diglot weave marks unmapped words as "No_Sub" (case may
                # vary). Show a simple placeholder instead of the raw marker.
                if gloss.lower() == _NO_SUB_TOKEN:
                    gloss = NO_SUB_GLYPH
                sent_cells.append(Cell(sp=sp, en=gloss, sent_n=n))
            elif "p" in tok:
                p = tok["p"].strip()
                if not p:
                    continue
                if sent_cells:
                    sent_cells[-1].sp += p
                else:
                    pending_prefix += p
        cells.extend(sent_cells)
    return cells


# ---------------------------------------------------------------------------
# Word timing via faster-whisper + sequence alignment
# ---------------------------------------------------------------------------
_WORD_CLEAN_RE = re.compile(r"[^\w]+", re.UNICODE)


def _norm_word(w: str) -> str:
    """Normalize a word for matching: lowercase, strip accents/punctuation."""
    w = unicodedata.normalize("NFKD", w)
    w = "".join(c for c in w if not unicodedata.combining(c))
    return _WORD_CLEAN_RE.sub("", w.lower())


def _reference_words(cells: list[Cell]) -> list[str]:
    """Flatten cells into individual reference words; record index ranges."""
    ref: list[str] = []
    for cell in cells:
        words = cell.sp.split()
        cell.ref_lo = len(ref)
        ref.extend(words)
        cell.ref_hi = len(ref) - 1
    return ref


def _transcribe_words(audio_file: Path, language: str, model_size: str) -> tuple[list, float]:
    """Run faster-whisper and return ([(word, start, end)], audio_duration)."""
    from faster_whisper import WhisperModel

    print(f"  -> [CC] Loading faster-whisper model '{model_size}' (cpu/int8)...")
    model = WhisperModel(model_size, device="cpu", compute_type="int8")
    print(f"  -> [CC] Transcribing '{audio_file.name}' for word timing (language={language})...")
    segments, info = model.transcribe(
        str(audio_file),
        language=language,
        word_timestamps=True,
        beam_size=5,
        condition_on_previous_text=False,
        vad_filter=False,
    )
    words: list[tuple[str, float, float]] = []
    for seg in segments:
        for w in seg.words or []:
            words.append((w.word.strip(), w.start, w.end))
    print(f"  -> [CC] Transcription produced {len(words)} words "
          f"(audio {info.duration:.1f}s).")
    return words, info.duration


def _align_timings(
    ref_words: list[str],
    hyp_words: list[tuple[str, float, float]],
    audio_duration: float,
) -> list[tuple[float, float]]:
    """Assign (start, end) to every reference word.

    Matched words take whisper timestamps; unmatched stretches are
    interpolated between surrounding anchors proportionally to word length.
    """
    ref_norm = [_norm_word(w) for w in ref_words]
    hyp_norm = [_norm_word(w) for (w, _, _) in hyp_words]

    times: list[tuple[float, float] | None] = [None] * len(ref_words)
    sm = SequenceMatcher(None, ref_norm, hyp_norm, autojunk=False)
    matched = 0
    for a, b, size in sm.get_matching_blocks():
        for k in range(size):
            times[a + k] = (hyp_words[b + k][1], hyp_words[b + k][2])
            matched += 1
    if ref_words:
        pct = 100.0 * matched / len(ref_words)
        print(f"  -> [CC] Word alignment: {matched}/{len(ref_words)} matched ({pct:.1f}%).")
        if pct < 50.0:
            print("  -> [CC] WARNING: low match rate; highlight timing may be inaccurate.")

    # Interpolate gaps
    n = len(times)
    i = 0
    while i < n:
        if times[i] is not None:
            i += 1
            continue
        gap_start = i
        while i < n and times[i] is None:
            i += 1
        gap_end = i  # exclusive
        t0 = times[gap_start - 1][1] if gap_start > 0 else 0.0
        t1 = times[gap_end][0] if gap_end < n else audio_duration
        if t1 < t0:
            t1 = t0
        weights = [max(len(ref_words[k]), 1) for k in range(gap_start, gap_end)]
        total = sum(weights)
        acc = 0.0
        for j, k in enumerate(range(gap_start, gap_end)):
            s = t0 + (t1 - t0) * (acc / total)
            acc += weights[j]
            e = t0 + (t1 - t0) * (acc / total)
            times[k] = (s, e)

    # Enforce monotonicity
    out: list[tuple[float, float]] = []
    last = 0.0
    for s, e in times:  # type: ignore[misc]
        s = max(s, last)
        e = max(e, s)
        out.append((s, e))
        last = e
    return out


def _timing_cache_path(output_dir: Path, audio_file: Path) -> Path:
    return output_dir / f"{audio_file.stem}_cc_timing.json"


def compute_cell_timings(
    cells: list[Cell],
    audio_file: Path,
    language: str,
    model_size: str,
    output_dir: Path,
    force: bool = False,
) -> bool:
    """Fill in cell.start / cell.end. Returns True on success.

    Results are cached next to the video output; the cache is invalidated
    when the audio file or the cell text changes.
    """
    ref_words = _reference_words(cells)
    stat = audio_file.stat()
    meta = {
        "audio_size": stat.st_size,
        "audio_mtime": int(stat.st_mtime),
        "model": model_size,
        "n_ref_words": len(ref_words),
    }

    cache_path = _timing_cache_path(output_dir, audio_file)
    if not force and cache_path.exists():
        try:
            with open(cache_path, encoding="utf-8") as f:
                cached = json.load(f)
            if cached.get("meta") == meta and len(cached.get("cells", [])) == len(cells):
                for cell, (s, e) in zip(cells, cached["cells"]):
                    cell.start, cell.end = s, e
                print(f"  -> [CC] Reusing cached word timings: {cache_path.name}")
                return True
        except Exception:
            pass

    try:
        hyp_words, audio_duration = _transcribe_words(audio_file, language, model_size)
    except Exception as e:
        print(f"  -> [CC] ERROR: word-timing transcription failed: {e}")
        return False
    if not hyp_words:
        print("  -> [CC] ERROR: transcription produced no words.")
        return False

    times = _align_timings(ref_words, hyp_words, audio_duration)
    for cell in cells:
        cell.start = times[cell.ref_lo][0]
        cell.end = times[cell.ref_hi][1]

    try:
        with open(cache_path, "w", encoding="utf-8") as f:
            json.dump({"meta": meta, "cells": [(c.start, c.end) for c in cells]}, f)
    except Exception as e:
        print(f"  -> [CC] WARNING: could not write timing cache: {e}")
    return True


# ---------------------------------------------------------------------------
# Font measurement (PIL) and row layout
# ---------------------------------------------------------------------------
def cc_band_height(video_h: int) -> int:
    """Height (px) of the caption area at the bottom of the frame: the three
    interlinear row slots plus margins. Used by create_video.py to size the
    frosted-glass backdrop behind the captions."""
    scale = video_h / DESIGN_H
    sp_size = round(SP_FONT_SIZE * scale)
    en_size = round(EN_FONT_SIZE * scale)
    row_h = sp_size + LINE_GAP * scale + en_size
    band = (3 * row_h + 2 * ROW_GAP * scale
            + BOTTOM_MARGIN * scale + BAND_TOP_PAD * scale)
    return min(video_h, int(round(band)))


_FONT_CANDIDATES_BOLD = [
    r"C:\Windows\Fonts\arialbd.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
]
_FONT_CANDIDATES_REG = [
    r"C:\Windows\Fonts\arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
]


class _Measurer:
    def __init__(self, size: int, bold: bool):
        self.size = size
        self.font = None
        try:
            from PIL import ImageFont
            for cand in (_FONT_CANDIDATES_BOLD if bold else _FONT_CANDIDATES_REG):
                if Path(cand).exists():
                    self.font = ImageFont.truetype(cand, size)
                    break
        except Exception:
            self.font = None

    def width(self, text: str) -> float:
        if self.font is not None:
            try:
                return float(self.font.getlength(text))
            except Exception:
                pass
        return 0.55 * self.size * len(text)  # rough fallback


def layout_rows(cells: list[Cell], video_w: int, video_h: int) -> list[Row]:
    """Assign widths / x positions to cells and wrap them into rows.

    Rows never cross sentence boundaries; each row is centered horizontally.
    """
    scale = video_h / DESIGN_H
    sp_size = round(SP_FONT_SIZE * scale)
    en_size = round(EN_FONT_SIZE * scale)
    gap = CELL_GAP * scale
    usable = video_w - 2 * SIDE_MARGIN * scale

    m_sp = _Measurer(sp_size, bold=True)
    m_en = _Measurer(en_size, bold=False)

    for cell in cells:
        cell.width = max(m_sp.width(cell.sp), m_en.width(cell.en) if cell.en else 0.0)

    rows: list[Row] = []
    cur: list[Cell] = []
    cur_w = 0.0
    cur_sent = None

    def flush():
        nonlocal cur, cur_w
        if cur:
            total = sum(c.width for c in cur) + gap * (len(cur) - 1)
            x = (video_w - total) / 2.0
            for c in cur:
                c.x = x + c.width / 2.0
                x += c.width + gap
            rows.append(Row(cells=cur))
        cur, cur_w = [], 0.0

    for cell in cells:
        new_w = cur_w + (gap if cur else 0.0) + cell.width
        if cur and (cell.sent_n != cur_sent or new_w > usable):
            flush()
            new_w = cell.width
        cur.append(cell)
        cur_w = new_w
        cur_sent = cell.sent_n
    flush()
    return rows


# ---------------------------------------------------------------------------
# ASS generation
# ---------------------------------------------------------------------------
def _ass_time(t: float) -> str:
    t = max(t, 0.0)
    cs = int(round(t * 100))
    h, rem = divmod(cs, 360000)
    m, rem = divmod(rem, 6000)
    s, cs = divmod(rem, 100)
    return f"{h}:{m:02d}:{s:02d}.{cs:02d}"


def _esc(text: str) -> str:
    return text.replace("{", "(").replace("}", ")").replace("\n", " ")


def build_ass(
    rows: list[Row],
    video_w: int,
    video_h: int,
    total_duration: float,
) -> str:
    """Emit the ASS script: 3 visible interlinear rows, current in middle."""
    scale = video_h / DESIGN_H
    sp_size = round(SP_FONT_SIZE * scale)
    en_size = round(EN_FONT_SIZE * scale)
    line_gap = LINE_GAP * scale
    row_gap = ROW_GAP * scale
    bottom_margin = BOTTOM_MARGIN * scale
    outline = max(2.0 * scale, 1.5)

    row_h = sp_size + line_gap + en_size
    # Slot y centers (an5) for the sp line and en line of each of 3 slots.
    slots = []
    y_bottom = video_h - bottom_margin
    for slot in (2, 1, 0):  # build from bottom slot upward
        en_cy = y_bottom - en_size / 2.0
        sp_cy = y_bottom - en_size - line_gap - sp_size / 2.0
        slots.append((slot, sp_cy, en_cy))
        y_bottom -= row_h + row_gap
    slot_y = {slot: (sp_cy, en_cy) for slot, sp_cy, en_cy in slots}

    header = f"""[Script Info]
; Generated by cc_subtitles.py (WeaveLang interlinear CC)
ScriptType: v4.00+
PlayResX: {video_w}
PlayResY: {video_h}
ScaledBorderAndShadow: yes
WrapStyle: 2

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: SP,{FONT_NAME},{sp_size},{COL_WHITE},{COL_WHITE},{COL_OUTLINE},{COL_OUTLINE},-1,0,0,0,100,100,0,0,1,{outline:.1f},0,5,0,0,0,1
Style: EN,{FONT_NAME},{en_size},{COL_GLOSS},{COL_GLOSS},{COL_OUTLINE},{COL_OUTLINE},0,0,0,0,100,100,0,0,1,{outline:.1f},0,5,0,0,0,1
Style: SPHL,{FONT_NAME},{sp_size},{COL_YELLOW},{COL_YELLOW},{COL_OUTLINE},{COL_OUTLINE},-1,0,0,0,100,100,0,0,1,{outline:.1f},0,5,0,0,0,1
Style: ENHL,{FONT_NAME},{en_size},{COL_YELLOW},{COL_YELLOW},{COL_OUTLINE},{COL_OUTLINE},0,0,0,0,100,100,0,0,1,{outline:.1f},0,5,0,0,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"""

    events: list[str] = []

    def emit(layer: int, start: float, end: float, style: str,
             x: float, y: float, text: str, alpha: str | None = None,
             fade: bool = True):
        if end - start < 0.01 or not text:
            return
        tags = f"\\an5\\pos({x:.0f},{y:.0f})"
        if alpha:
            tags += f"\\alpha{alpha}"
        if fade:
            tags += f"\\fad({FADE_MS},{FADE_MS})"
        events.append(
            f"Dialogue: {layer},{_ass_time(start)},{_ass_time(end)},{style},,0,0,0,,"
            f"{{{tags}}}{_esc(text)}"
        )

    # Window i: row i is current, in [t_i, t_{i+1})
    n = len(rows)
    for i, row in enumerate(rows):
        w_start = rows[i].start if i > 0 else 0.0
        w_end = rows[i + 1].start if i + 1 < n else total_duration
        if w_end <= w_start:
            continue

        # top slot: previous row, fading away
        if i > 0:
            sp_y, en_y = slot_y[0]
            for c in rows[i - 1].cells:
                emit(0, w_start, w_end, "SP", c.x, sp_y, c.sp, alpha=ALPHA_TOP)
                emit(0, w_start, w_end, "EN", c.x, en_y, c.en, alpha=ALPHA_TOP)

        # middle slot: current row (white base + yellow highlight overlay)
        sp_y, en_y = slot_y[1]
        for c in row.cells:
            emit(0, w_start, w_end, "SP", c.x, sp_y, c.sp)
            emit(0, w_start, w_end, "EN", c.x, en_y, c.en)
            h_start = max(c.start, w_start)
            h_end = min(max(c.end, c.start + 0.15), w_end)
            emit(1, h_start, h_end, "SPHL", c.x, sp_y, c.sp, fade=False)
            emit(1, h_start, h_end, "ENHL", c.x, en_y, c.en, fade=False)

        # bottom slot: upcoming row, slightly dimmed
        if i + 1 < n:
            sp_y, en_y = slot_y[2]
            for c in rows[i + 1].cells:
                emit(0, w_start, w_end, "SP", c.x, sp_y, c.sp, alpha=ALPHA_BOTTOM)
                emit(0, w_start, w_end, "EN", c.x, en_y, c.en, alpha=ALPHA_BOTTOM)

    return header + "\n".join(events) + "\n"


# ---------------------------------------------------------------------------
# Public entry point
# ---------------------------------------------------------------------------
def sentence_range_from_chunk_map(chunks_dir: Path | None) -> tuple[int, int] | None:
    """Derive the global sentence range covered by this audio from the chunk
    map (supports volume files that cover a subset of the book)."""
    if not chunks_dir:
        return None
    chunk_map_path = chunks_dir / "_chunk_map.json"
    if not chunk_map_path.exists():
        return None
    try:
        with open(chunk_map_path, encoding="utf-8") as f:
            chunk_map = json.load(f)
        chunks = chunk_map.get("chunks", [])
        if not chunks:
            return None
        lo = min(c["start_sentence"] for c in chunks)
        hi = max(c["end_sentence"] for c in chunks)
        return (lo, hi)
    except Exception:
        return None


def build_cc_ass(
    audio_file: Path,
    cc_map_path: Path,
    output_dir: Path,
    video_w: int,
    video_h: int,
    audio_duration: float,
    whisper_model: str = "small",
    chunks_dir: Path | None = None,
    force_align: bool = False,
) -> Path | None:
    """Build the interlinear CC .ass file. Returns its path, or None on failure."""
    try:
        cc_map = load_cc_map(cc_map_path)
    except Exception as e:
        print(f"  -> [CC] ERROR: cannot load CC map '{cc_map_path}': {e}")
        return None

    sent_range = sentence_range_from_chunk_map(chunks_dir)
    if sent_range:
        print(f"  -> [CC] Audio covers sentences {sent_range[0]}..{sent_range[1]} (from chunk map).")

    cells = build_cells(cc_map, sent_range)
    if not cells:
        print("  -> [CC] ERROR: CC map contains no word tokens.")
        return None
    print(f"  -> [CC] {len(cells)} interlinear cells from {cc_map_path.name}.")

    language = cc_map.get("lang_spoken", "es")
    if not compute_cell_timings(cells, audio_file, language, whisper_model,
                                output_dir, force=force_align):
        return None

    rows = layout_rows(cells, video_w, video_h)
    print(f"  -> [CC] Layout: {len(rows)} interlinear rows at {video_w}x{video_h}.")

    ass_text = build_ass(rows, video_w, video_h, audio_duration)
    ass_path = output_dir / f"{audio_file.stem}_cc.ass"
    with open(ass_path, "w", encoding="utf-8-sig") as f:
        f.write(ass_text)
    print(f"  -> [CC] Wrote subtitles: {ass_path.name}")
    return ass_path


def main():
    parser = argparse.ArgumentParser(description="Build interlinear CC .ass subtitles.")
    parser.add_argument("--audio-file", type=Path, required=True)
    parser.add_argument("--cc-map", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--chunks-dir", type=Path, default=None)
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument("--height", type=int, default=720)
    parser.add_argument("--model", type=str, default="small")
    parser.add_argument("--force-align", action="store_true")
    args = parser.parse_args()

    from pydub import AudioSegment
    duration = len(AudioSegment.from_file(args.audio_file)) / 1000.0

    args.output_dir.mkdir(parents=True, exist_ok=True)
    result = build_cc_ass(
        args.audio_file, args.cc_map, args.output_dir,
        args.width, args.height, duration,
        whisper_model=args.model, chunks_dir=args.chunks_dir,
        force_align=args.force_align,
    )
    sys.exit(0 if result else 1)


if __name__ == "__main__":
    main()
