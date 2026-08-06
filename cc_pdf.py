"""Interlinear read-along PDF builder for WeaveLang.

Consumes the CC map JSON exported by the Rust app during ``generate_weave b``
(``<stem>_cc.json`` in tts_files/) and typesets a printable interlinear
reader: each spoken (target-language) word or phrase sits above its
base-language gloss, flowing continuously like book text.

Typography notes:
  - Letter page, generous margins, classic serif (Palatino Linotype when
    available, then Constantia, Georgia, or the built-in Times faces).
  - Spoken text in ink-black roman; glosses in smaller, warm-gray italic so
    the target language stays visually dominant.
  - Small raised sentence numbers for navigating alongside the audio.
  - Title (and chapter, in chapter mode) heading with a short centered rule;
    running header and centered folios from page two onward.

Standalone usage:
    python cc_pdf.py --cc-map X_cc.json [--output out.pdf]
        [--title "El Sombrerón"] [--chapter "Chapter 1"]

When --title is omitted it is derived from the file name: underscores become
spaces and the trailing ``_ULxNN_cc`` suffix is dropped.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

from reportlab.lib.pagesizes import letter
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.pdfgen.canvas import Canvas

from cc_subtitles import NO_SUB_GLYPH, Cell, build_cells, load_cc_map

# ---------------------------------------------------------------------------
# Page geometry (points)
# ---------------------------------------------------------------------------
PAGE_W, PAGE_H = letter
MARGIN_L = 80
MARGIN_R = 80
MARGIN_TOP = 88
MARGIN_BOTTOM = 76
CONTENT_W = PAGE_W - MARGIN_L - MARGIN_R

# ---------------------------------------------------------------------------
# Type scale
# ---------------------------------------------------------------------------
ES_SIZE = 11.5          # spoken (target-language) words
GL_SIZE = 7.8           # gloss line
NUM_SIZE = 6.2          # raised sentence numbers
TITLE_SIZE = 25
CHAPTER_SIZE = 13
HEADER_SIZE = 7.5       # running header (letterspaced caps)
FOLIO_SIZE = 9

INTRA_GAP = 3.0         # vertical gap between word baseline box and gloss
ROW_LEAD = 14.0         # vertical space between interlinear rows
CELL_GAP = 13.0         # horizontal gap between word cells
SENT_EXTRA = 9.0        # extra horizontal breathing room after a sentence
NUM_PAD = 2.5           # gap between sentence number and first word

ROW_H = ES_SIZE + INTRA_GAP + GL_SIZE + ROW_LEAD

# Ink colors (grayscale; warm and restrained)
INK = 0.12              # spoken text
GLOSS_GRAY = 0.46       # gloss text
NUM_GRAY = 0.55         # sentence numbers
RULE_GRAY = 0.62        # heading rule
HEADER_GRAY = 0.5       # running header / folio


# ---------------------------------------------------------------------------
# Fonts
# ---------------------------------------------------------------------------
_FONT_CANDIDATES = [
    # (regular, bold, italic) file names under C:\Windows\Fonts
    ("pala.ttf", "palab.ttf", "palai.ttf"),        # Palatino Linotype
    ("constan.ttf", "constanb.ttf", "constani.ttf"),  # Constantia
    ("georgia.ttf", "georgiab.ttf", "georgiai.ttf"),  # Georgia
]

SERIF = "Times-Roman"
SERIF_BOLD = "Times-Bold"
SERIF_ITALIC = "Times-Italic"


def _register_fonts() -> None:
    """Register the best available serif family as Serif/-Bold/-Italic."""
    global SERIF, SERIF_BOLD, SERIF_ITALIC
    fonts_dir = Path("C:/Windows/Fonts")
    for regular, bold, italic in _FONT_CANDIDATES:
        reg_p, bold_p, ital_p = (fonts_dir / regular, fonts_dir / bold, fonts_dir / italic)
        if reg_p.exists() and bold_p.exists() and ital_p.exists():
            try:
                pdfmetrics.registerFont(TTFont("Serif", str(reg_p)))
                pdfmetrics.registerFont(TTFont("Serif-Bold", str(bold_p)))
                pdfmetrics.registerFont(TTFont("Serif-Italic", str(ital_p)))
                SERIF, SERIF_BOLD, SERIF_ITALIC = "Serif", "Serif-Bold", "Serif-Italic"
                return
            except Exception:
                continue
    # Fall back to the built-in Times faces (already assigned).


def _w(text: str, font: str, size: float) -> float:
    return pdfmetrics.stringWidth(text, font, size)


# ---------------------------------------------------------------------------
# Layout
# ---------------------------------------------------------------------------
class PlacedCell:
    """A cell measured and positioned within a wrapped line."""

    __slots__ = ("cell", "x", "width", "number")

    def __init__(self, cell: Cell, x: float, width: float, number: str | None):
        self.cell = cell
        self.x = x          # left edge relative to content origin
        self.width = width  # cell width (word/gloss max), excluding number
        self.number = number  # sentence number drawn before the cell, or None


def _layout_lines(cells: list[Cell], numbering: bool = True) -> list[list[PlacedCell]]:
    """Wrap interlinear cells into lines of CONTENT_W points."""
    lines: list[list[PlacedCell]] = []
    line: list[PlacedCell] = []
    x = 0.0
    prev_sent = None

    for cell in cells:
        gloss = "" if cell.en == NO_SUB_GLYPH else cell.en
        es_w = _w(cell.sp, SERIF, ES_SIZE)
        gl_w = _w(gloss, SERIF_ITALIC, GL_SIZE) if gloss else 0.0
        cell_w = max(es_w, gl_w)

        number = None
        prefix_w = 0.0
        if cell.sent_n != prev_sent:
            if prev_sent is not None:
                x += SENT_EXTRA
            if numbering:
                number = str(cell.sent_n)
                prefix_w = _w(number, SERIF, NUM_SIZE) + NUM_PAD
            prev_sent = cell.sent_n

        total_w = prefix_w + cell_w
        if line and x + total_w > CONTENT_W:
            lines.append(line)
            line = []
            x = 0.0

        line.append(PlacedCell(cell, x + prefix_w, cell_w, number))
        x += total_w + CELL_GAP

    if line:
        lines.append(line)
    return lines


# ---------------------------------------------------------------------------
# Drawing
# ---------------------------------------------------------------------------
def _draw_first_page_heading(c: Canvas, title: str, chapter: str | None) -> float:
    """Draw the title block; return the y coordinate where body text starts."""
    y = PAGE_H - MARGIN_TOP

    c.setFillGray(INK)
    c.setFont(SERIF_BOLD, TITLE_SIZE)
    c.drawCentredString(PAGE_W / 2, y - TITLE_SIZE, title)
    y -= TITLE_SIZE + 14

    if chapter:
        c.setFillGray(GLOSS_GRAY)
        c.setFont(SERIF_ITALIC, CHAPTER_SIZE)
        c.drawCentredString(PAGE_W / 2, y - CHAPTER_SIZE, chapter)
        y -= CHAPTER_SIZE + 14

    # Short centered rule — a quiet separation between heading and text.
    rule_w = CONTENT_W * 0.28
    c.setStrokeGray(RULE_GRAY)
    c.setLineWidth(0.6)
    c.line(PAGE_W / 2 - rule_w / 2, y - 4, PAGE_W / 2 + rule_w / 2, y - 4)

    return y - 34


def _draw_running_header(c: Canvas, title: str) -> None:
    # Letterspaced caps, tracked manually per glyph (ReportLab's charSpace
    # state leaks from text objects into subsequent drawString calls).
    text = title.upper()
    char_space = 1.4
    width = _w(text, SERIF, HEADER_SIZE) + char_space * max(len(text) - 1, 0)
    x = PAGE_W / 2 - width / 2
    y = PAGE_H - 46
    c.setFillGray(HEADER_GRAY)
    c.setFont(SERIF, HEADER_SIZE)
    for ch in text:
        c.drawString(x, y, ch)
        x += _w(ch, SERIF, HEADER_SIZE) + char_space


def _draw_folio(c: Canvas, page_num: int) -> None:
    c.setFillGray(HEADER_GRAY)
    c.setFont(SERIF, FOLIO_SIZE)
    c.drawCentredString(PAGE_W / 2, 42, str(page_num))


def _draw_line(c: Canvas, line: list[PlacedCell], y_top: float) -> None:
    """Draw one interlinear row whose top edge is at y_top."""
    es_baseline = y_top - ES_SIZE
    gl_baseline = es_baseline - INTRA_GAP - GL_SIZE

    for placed in line:
        x = MARGIN_L + placed.x
        if placed.number:
            c.setFillGray(NUM_GRAY)
            c.setFont(SERIF, NUM_SIZE)
            num_w = _w(placed.number, SERIF, NUM_SIZE)
            c.drawString(x - num_w - NUM_PAD, es_baseline + 3, placed.number)

        c.setFillGray(INK)
        c.setFont(SERIF, ES_SIZE)
        c.drawString(x, es_baseline, placed.cell.sp)

        gloss = "" if placed.cell.en == NO_SUB_GLYPH else placed.cell.en
        if gloss:
            c.setFillGray(GLOSS_GRAY)
            c.setFont(SERIF_ITALIC, GL_SIZE)
            c.drawString(x, gl_baseline, gloss)


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------
def derive_title(cc_map_path: Path) -> str:
    """Derive a display title from a cc-map file name.

    ``El_Sombrerón_ULb27_cc`` → ``El Sombrerón``.
    """
    stem = cc_map_path.stem
    stem = re.sub(r"_cc$", "", stem)
    stem = re.sub(r"_UL[a-z]*\d*$", "", stem)
    return stem.replace("_", " ").strip()


def default_output_path(cc_map_path: Path) -> Path:
    """Place the PDF beside the weave outputs (parent of tts_files/)."""
    stem = re.sub(r"_cc$", "", cc_map_path.stem)
    parent = cc_map_path.parent
    if parent.name == "tts_files":
        parent = parent.parent
    return parent / f"{stem}_reader.pdf"


def build_cc_pdf(
    cc_map_path: Path,
    output_path: Path | None = None,
    title: str | None = None,
    chapter: str | None = None,
) -> Path:
    """Typeset an interlinear read-along PDF from a CC map. Returns the path."""
    _register_fonts()

    cc_map = load_cc_map(cc_map_path)
    cells = build_cells(cc_map)
    if not cells:
        raise ValueError(f"No interlinear cells in '{cc_map_path}'.")

    if title is None:
        title = derive_title(cc_map_path)
    if output_path is None:
        output_path = default_output_path(cc_map_path)

    # If the text opens with a sentence that merely repeats the title
    # (a common pattern for short works), drop it — the heading covers it.
    first_n = cells[0].sent_n
    first_sent_text = " ".join(c.sp for c in cells if c.sent_n == first_n)
    if first_sent_text.strip().rstrip(".").casefold() == title.casefold():
        cells = [c for c in cells if c.sent_n != first_n]
        if not cells:
            raise ValueError("CC map contains only the title sentence.")

    lines = _layout_lines(cells)

    output_path.parent.mkdir(parents=True, exist_ok=True)
    c = Canvas(str(output_path), pagesize=letter)
    c.setTitle(title if not chapter else f"{title} — {chapter}")
    c.setAuthor("WeaveLang")
    c.setSubject("Interlinear read-along text")

    page_num = 1
    y = _draw_first_page_heading(c, title, chapter)

    for line in lines:
        if y - ROW_H < MARGIN_BOTTOM:
            _draw_folio(c, page_num)
            c.showPage()
            page_num += 1
            _draw_running_header(c, title)
            y = PAGE_H - MARGIN_TOP
        _draw_line(c, line, y)
        y -= ROW_H

    _draw_folio(c, page_num)
    c.showPage()
    c.save()
    return output_path


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Typeset an interlinear read-along PDF from a WeaveLang CC map."
    )
    parser.add_argument("--cc-map", type=Path, required=True,
                        help="Path to the *_cc.json interlinear CC map.")
    parser.add_argument("--output", type=Path, default=None,
                        help="Output PDF path (default: <stem>_reader.pdf beside the weave outputs).")
    parser.add_argument("--title", type=str, default=None,
                        help="Story title (default: derived from the file name).")
    parser.add_argument("--chapter", type=str, default=None,
                        help="Chapter heading shown beneath the title (chapter mode).")
    args = parser.parse_args()

    if not args.cc_map.exists():
        print(f"Error: CC map not found: {args.cc_map}")
        return 1

    try:
        out = build_cc_pdf(args.cc_map, args.output, args.title, args.chapter)
    except Exception as e:
        print(f"Error: PDF build failed: {e}")
        return 1

    print(f"Interlinear reader PDF -> {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
