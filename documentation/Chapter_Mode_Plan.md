# Chapter Mode Plan

## Overview

Producing a whole book costs ~$500 (mostly TTS, ~$50 for the weaveLang build itself). To enable pre-releasing chapters to YouTube while the book is still in progress, we add **Chapter Mode** — the ability to define, calibrate, and weave individual chapters independently.

A chapter is a named range of sentences. The user defines chapters, calibrates on whatever chapters are complete, and weaves individual chapters with correct book-global level progression.

---

## Phase 1 — Core

### Data Model & Persistence

- [x] Add `Chapter` struct: `name: String`, `start: usize`, `end: usize` (1-based, inclusive)
- [x] Add `chapters: Vec<Chapter>` to `AppState`
- [x] Add `chapter_mode: bool` to `AppState` (default `false`)
- [x] Add `selected_chapter_idx: Option<usize>` to `AppState`
- [x] Persist chapters to `_chapters.toml` in the project directory
- [x] Load chapters on project open
- [x] Sanitize chapter names: replace spaces with underscores in directory/file names (same as book name handling)

### Commands

- [x] `new chapter "<name>" <start> <end>` — validate no overlapping ranges, push to `Vec<Chapter>`
- [x] `list chapters` — print chapters sorted by start index, showing name, range, and validity status (all sentences in range have 5 tiers + both mappings)
- [x] `delete chapter "<name>"` — remove by name
- [x] `select chapter "<name>"` — set `selected_chapter_idx`
- [x] `set chapter_mode true|false` — toggle mode; persist in config
- [x] Add command parsing in `terminal.rs`
- [x] Add command execution in `engine.rs`

### Calibration on Incomplete Data

- [x] When `calibrate` is run in chapter mode, collect all sentences from chapters marked as complete (valid)
- [x] Build a synthetic `JsonChapter` preserving original book-global sentence indices (insert empty placeholders for gaps between chapters)
- [x] Pass full book sentence count as `total_sentences` so `start_sentence_idx` values remain book-global
- [x] Feed synthetic chapter to existing `calibrate_from_chapter()`
- [x] Resulting level map has correct global sentence indices — recipes are approximate but close enough

### Chapter-Scoped Weave Generation

- [x] In chapter mode, `generate weave` operates on the selected chapter only
- [x] Filter recipe entries to those whose `start_sentence_idx` falls within the chapter's range
- [x] Recipe values are looked up by book-global sentence index (e.g., chapter starts at sentence 456 → use recipe for sentence 456, not sentence 1)
- [x] Output filename: `BookName_ChapterName_UL07-10.txt` (spaces replaced with underscores)

### Directory Structure & `init media workspace`

- [x] Add `init media workspace` command
- [x] Create the following structure (auto-create missing directories):

```
weave_output/
  Book_Name/
    _chapters.toml
    _av_manifest.toml
    whole_book/
      tts_files/
        BookName_UL01.txt
        BookName_UL02.txt
        ...
      audio/
      video/
      illustrations/
    The_Fallen_Prince/
      _av_manifest.toml
      tts_files/
        BookName_The_Fallen_Prince_UL01.txt
        ...
      audio/
        chunks/
      video/
      illustrations/
```

- [x] TTS text files go in `tts_files/` subdirectory (consistent with audio/video/illustrations pattern, avoids clutter at the chapter level)
- [x] Auto-create directories when generating output (extend existing `fs::create_dir_all` usage)

### Media View — Chapter Scoping

- [x] In chapter mode, `AvProducer` is constructed with the chapter's subdirectory as `book_dir`
- [x] `scan()` automatically shows only that chapter's stems (directory-based discovery unchanged)
- [x] In book mode, `AvProducer` uses `whole_book/` subdirectory
- [ ] Add `Init Media Workspace` button to the Media view

### GUI — Preferences Toggle

- [x] Add to Preferences menu: two radio-style items (`Book Mode` ✓ / `Chapter Mode`) using `ui.radio_value()` or `ui.selectable_value()`
- [x] Selecting either emits `set chapter_mode true|false`

### GUI — Chapters Menu

- [x] Add new top-level `Chapters` menu to the menu bar
- [x] `Select Chapter ▸` submenu listing all defined chapters
- [x] `New Chapter...` menu item (opens dialog or emits command)
- [x] `Delete Chapter...` menu item
- [x] `Init Media Workspace` menu item
- [ ] Menu is visible in both modes but items are disabled/greyed when `chapter_mode` is `false`

---

## Phase 2 — Polish

### Focus View on Chapter

- [ ] Add `Focus View on Chapter` toggle to Chapters menu (checkbox style)
- [ ] When active, navigator filters sentence list to only the selected chapter's range
- [ ] Detail view, keyboard navigation, and sentence selection respect the filtered range
- [ ] Deselecting restores full book view

### Chapter Progress Indicators

- [ ] Show chapter completion percentage in navigator (e.g., "The Fallen Prince: 72/85 ready")
- [ ] Color-code chapters in the Chapters menu by validity

### Additional Commands

- [ ] `rename chapter "<old>" "<new>"` — rename chapter and move its directory
- [ ] Auto-reindex chapter ranges on sentence insert/delete (or warn user)

---

## Edge Cases

| Issue | Approach |
|-------|----------|
| Overlapping chapter ranges | Reject at `new chapter` time — range collision check |
| Gaps between chapters | Allowed — not every sentence needs a chapter |
| Chapter range exceeds document length | Warn but allow (book is still growing) |
| Recalibration after adding chapters | Level map is regenerated; flag stale weave outputs |
| Sentence insert/delete shifts ranges | Phase 2: auto-adjust or `reindex chapters` command |
| Spaces in names | Replace with underscores in file/directory names |

---

## Design Notes

- **Book mode is default.** The app behaves exactly as it does today unless `chapter_mode` is enabled.
- **Recipes are approximate on partial data.** Vocabulary frequency distributions stabilize quickly. Even one chapter produces a level map whose recipes are close to the final whole-book map. The difference is unlikely to be noticeable to a student.
- **Global sentence indexing is preserved.** A chapter starting at sentence 456 uses the recipe for sentence 456 in the level map, not sentence 1. This ensures chapters maintain correct relative difficulty progression as if the whole book were output.
- **The calibrator already accepts `JsonChapter`** — no architectural changes needed, just a chapter-aware "gather sentences" step before calling it.
- **Media view is directory-based** — scoping it to a chapter subdirectory is near-free.
