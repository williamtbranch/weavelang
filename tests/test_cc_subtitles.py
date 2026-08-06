"""Quick smoke test for cc_subtitles.py layout + ASS generation (no whisper).

Builds a tiny synthetic CC map, fakes cell timings, and checks the ASS output
structure. Run: python tests/test_cc_subtitles.py
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from cc_subtitles import (
    Cell, build_cells, layout_rows, build_ass, _align_timings, _norm_word,
)


def test_build_cells():
    cc_map = {
        "format": 1,
        "lang_spoken": "es",
        "lang_gloss": "en",
        "sentences": [
            {"n": 1, "text": "Si dejas el camino, puedes caer.", "tokens": [
                {"w": "Si", "g": "If"},
                {"w": "dejas", "g": "you leave"},
                {"w": "el", "g": "the"},
                {"w": "camino", "g": "path"},
                {"p": ","},
                {"w": "puedes caer", "g": "you might fall"},
                {"p": "."},
            ]},
            {"n": 2, "text": '"Ven aquí."', "tokens": [
                {"p": '"'},
                {"w": "Ven", "g": "Come"},
                {"w": "aquí", "g": "here"},
                {"p": '."'},
            ]},
        ],
    }
    cells = build_cells(cc_map)
    assert len(cells) == 7, [c.sp for c in cells]
    assert cells[3].sp == "camino,"          # punct attached to previous
    assert cells[4].sp == "puedes caer."
    assert cells[4].en == "you might fall"
    assert cells[5].sp == '"Ven'             # opening punct prefixed
    assert cells[6].sp == 'aquí."'
    assert cells[5].sent_n == 2
    print("test_build_cells OK")
    return cells


def test_no_sub_placeholder():
    cc_map = {
        "format": 1, "lang_spoken": "es", "lang_gloss": "en",
        "sentences": [
            {"n": 1, "text": "x", "tokens": [
                {"w": "gorro", "g": "NO_SUB"},   # upper
                {"w": "rojo", "g": "no_sub"},    # lower
                {"w": "casa", "g": "house"},
            ]},
        ],
    }
    cells = build_cells(cc_map)
    assert cells[0].en == "~", cells[0].en
    assert cells[1].en == "~", cells[1].en
    assert cells[2].en == "house"
    print("test_no_sub_placeholder OK")


def test_align_timings():
    ref = ["Si", "dejas", "el", "camino", "puedes", "caer"]
    # whisper missed "el", mangled "camino" -> "camion"
    hyp = [("Si", 0.0, 0.2), ("dejas", 0.25, 0.6), ("camion", 0.8, 1.2),
           ("puedes", 1.3, 1.6), ("caer", 1.65, 2.0)]
    times = _align_timings(ref, hyp, 2.5)
    assert len(times) == 6
    assert times[0] == (0.0, 0.2)
    assert times[1] == (0.25, 0.6)
    # "el" and "camino" interpolated between 0.6 and 1.3, monotone
    assert 0.6 <= times[2][0] <= times[2][1] <= times[3][1] <= 1.3 + 1e-9
    assert times[4] == (1.3, 1.6)
    # monotonicity overall
    last = 0.0
    for s, e in times:
        assert s >= last - 1e-9 and e >= s
        last = e
    print("test_align_timings OK")


def test_layout_and_ass():
    cells = test_build_cells()
    # fake timings: 0.5s per cell
    t = 0.0
    for c in cells:
        c.start, c.end = t, t + 0.4
        t += 0.5
    rows = layout_rows(cells, 1280, 720)
    assert len(rows) >= 2  # sentence boundary forces a new row
    for row in rows:
        # cells within a row are horizontally ordered and non-overlapping
        xs = [c.x for c in row.cells]
        assert xs == sorted(xs)
        assert all(c.width > 0 for c in row.cells)
        assert len({c.sent_n for c in row.cells}) == 1  # no row crosses sentences

    ass = build_ass(rows, 1280, 720, total_duration=t + 1.0)
    assert "[Script Info]" in ass and "[Events]" in ass
    assert "Style: SP," in ass and "Style: SPHL," in ass
    assert "Dialogue:" in ass
    # highlight events exist (layer 1)
    hl = [l for l in ass.splitlines() if l.startswith("Dialogue: 1,")]
    assert hl, "no highlight events"
    assert any("SPHL" in l for l in hl)
    # every dialogue line carries a \pos tag
    dlg = [l for l in ass.splitlines() if l.startswith("Dialogue:")]
    assert all("\\pos(" in l for l in dlg)
    print(f"test_layout_and_ass OK ({len(rows)} rows, {len(dlg)} events)")


def test_norm_word():
    assert _norm_word("Camión,") == "camion"
    assert _norm_word("«aquí»") == "aqui"
    print("test_norm_word OK")


if __name__ == "__main__":
    test_norm_word()
    test_align_timings()
    test_no_sub_placeholder()
    test_layout_and_ass()
    print("All cc_subtitles smoke tests passed.")
