# AV Production Integration Plan

## Overview

Port the functionality of `b2a.ps1` (text-to-speech audio generation) and `c2v.ps1` (video generation from audio + illustrations) into the WeaveLang Rust application. All functionality is exposed as terminal commands; the GUI is a dumb frontend that emits those commands.

## Design Principles

1. **Terminal-first**: Every action is a terminal command. GUI buttons emit terminal commands via `pending_terminal_command`. This enables copilot/agent automation and power-user workflows.
2. **Config in workspace, not dev dir**: All settings live in workspace `config.toml` or the per-book `_av_manifest.toml`. No settings in the development directory. The app will eventually be deployed without a dev environment.
3. **API keys via app keyring**: WeaveLang already manages API keys for Claude and Gemini via `secrets.rs` / keyring. TTS uses the same Gemini credentials — no separate `.env` file needed.
4. **One file at a time**: Audio generation processes one file at a time (sequential). The `--concurrent-requests` arg defaults to 1. Concurrency option exists in the manifest but defaults conservatively.
5. **Subprocess model**: Audio and video generation invoke `book_to_audio.py` and `create_video.py` as child processes. This reuses existing tested Python code and avoids porting Google TTS SDK dependencies to Rust.
6. **Simple but useful**: No embedded audio player, no streaming progress parsing (initially). Status is filesystem-derived. The user gets a clear table of what exists and what doesn't.

---

## Directory Layout

All media lives under the book's output directory:

```
{output_dir}/{book_name}/
├── Metamorphosis_UL1-6.txt        # woven text (already produced by generate weave)
├── Metamorphosis_UL7-10.txt
├── Metamorphosis_UL11.txt
├── ...
├── _av_manifest.toml              # AV production config + marked file list
├── audio/
│   ├── Metamorphosis_UL7-10.wav   # final concatenated audio
│   ├── Metamorphosis_UL11.wav
│   └── chunks/
│       ├── Metamorphosis_UL7-10/
│       │   ├── chunk_001.txt      # source text for this chunk
│       │   ├── chunk_001.wav      # generated audio for this chunk
│       │   ├── chunk_002.txt
│       │   ├── chunk_002.wav
│       │   ├── chunk_003.txt
│       │   └── chunk_003.wav.bad  # rejected — needs regen
│       └── Metamorphosis_UL11/
│           ├── chunk_001.txt
│           ├── chunk_001.wav
│           └── ...
├── video/
│   ├── Metamorphosis_UL7-10.mp4
│   └── Metamorphosis_UL11.mp4
└── illustrations/
    ├── 001.png
    ├── 002.png
    └── ...
```

- **Woven text** files are the source of truth for what _could_ become audio/video.
- **`audio/`** and **`video/`** subdirectories are created on first generation.
- **`illustrations/`** must be populated externally (image generation is future work). The app checks for its existence before video generation.
- **`_av_manifest.toml`** tracks which files are marked for AV and stores TTS/video config.

---

## AV Manifest (`_av_manifest.toml`)

```toml
[tts]
service = "gemini"
model = "models/gemini-2.5-pro-preview-tts"
voices = ["Charon", "aoede", "Puck", "Zephyr", "Fenrir", "Kore", "Orus", "Leda"]
prompt_prefix = "You are a professional voice actor with a Mexican Spanish accent..."
use_vertex_auth = true
output_format = "wav"           # default; extensible later
chunk_max_chars = 5000
max_api_retries = 2
retry_delay = 20
concurrent_requests = 1         # default 1; option exists for future use

[video]
image_duration = 8              # seconds per illustration
frame_rate = 30

# Stems of woven text files marked for AV production.
# Only files listed here are shown as actionable in the Media tab.
# Status (text/audio/video existence) is always derived from the filesystem.
[files]
marked = [
    "Metamorphosis_UL7-10",
    "Metamorphosis_UL11",
    "Metamorphosis_UL12",
]
```

### Why a manifest?

- **Explicit marking** solves the "UL1-6 vs UL2-6" problem — the user decides which files are intended for AV production.
- **Status is never stored** — it's derived from filesystem presence at scan time. No stale state.
- **TTS config per book** — different books may have different voices, prompts, languages. This is stored alongside the book, not globally.

---

## Terminal Commands

### Status & Scanning

| Command | Description |
|---------|-------------|
| `av status` | Scan book directory, display table of all woven text files with text/audio/video existence. Marked files shown prominently, unmarked shown grayed. |
| `av scan` | Force rescan of filesystem (same as status but also refreshes cached state). |

### Marking Files

| Command | Description |
|---------|-------------|
| `av mark <stem> [stem2 ...]` | Add stem(s) to the manifest's marked list. |
| `av unmark <stem> [stem2 ...]` | Remove stem(s) from the manifest's marked list. |
| `av mark-all` | Mark all woven text files found in the book directory. |
| `av clear-marks` | Remove all marks (empty the marked list). |

`<stem>` is the filename without `.txt` extension, e.g. `Metamorphosis_UL11`.

### Audio Generation

| Command | Description |
|---------|-------------|
| `av generate audio <stem>` | Generate audio for a specific marked file. |
| `av generate audio next` | Generate audio for the next marked file that lacks audio. |
| `av generate audio all` | Sequentially generate audio for all marked files lacking audio. |

Audio generation:
1. Reads the woven text file from `{book_dir}/{stem}.txt`
2. Invokes `book_to_audio.py` with args derived from `_av_manifest.toml` [tts] section
3. Outputs to `{book_dir}/audio/{stem}.wav`
4. API key is retrieved from the app's keyring (passed as env var to subprocess)

### Video Generation

| Command | Description |
|---------|-------------|
| `av generate video <stem>` | Generate video for a specific file (requires audio + illustrations). |
| `av generate video next` | Generate video for the next file that has audio but lacks video. |
| `av generate video all` | Sequentially generate video for all eligible files. |

Video generation:
1. Requires `{book_dir}/audio/{stem}.wav` to exist
2. Requires `{book_dir}/illustrations/` to contain at least one image
3. Invokes `create_video.py` with appropriate args
4. Outputs to `{book_dir}/video/{stem}.mp4`

### Configuration

| Command | Description |
|---------|-------------|
| `av config show` | Display current manifest TTS and video settings. |
| `av config tts <key> <value>` | Set a TTS config value (service, model, voices, prompt_prefix, etc.). |
| `av config video <key> <value>` | Set a video config value (image_duration, frame_rate). |
| `av config voices <v1> [v2 ...]` | Set the voice list (convenience command for the array). |

### Utility

| Command | Description |
|---------|-------------|
| `av open book-dir` | Open the book output directory in the system file explorer. |
| `av open audio-dir` | Open the audio subdirectory in the system file explorer. |
| `av open video-dir` | Open the video subdirectory in the system file explorer. |
| `av init` | Create `_av_manifest.toml` with defaults if it doesn't exist. |
| `av cancel` | Cancel running AV generation subprocess. |

### Chunk Management

| Command | Description |
|---------|-------------|
| `av chunks <stem>` | Show chunk status for a stem (index, text, audio, rejected). |
| `av reject chunk <stem> <N>` | Mark chunk N as bad (renames `.wav` → `.wav.bad`). |
| `av restore chunk <stem> <N>` | Restore a rejected chunk (renames `.wav.bad` → `.wav`). |
| `av rebuild audio <stem>` | Concatenate all good chunks into final `audio/<stem>.wav`. |

---

## GUI: Media Tab

A new **"Media"** tab is added to the top bar alongside Source / Advanced / Moderate / Basic T / Basic B / Simulation.

### Layout

```
┌──────────────────────────────────────────────────────────────────────┐
│ Source | Advanced | Moderate | Basic T | Basic B | Sim | Media       │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  TTS: gemini / Charon,aoede,Puck...       [⚙ Config]                │
│  Illustrations: 12 found in /illustrations [📁 Open]                 │
│                                                                      │
│  ┌────────────────────────┬───────┬───────┬───────┬────────────────┐ │
│  │ File                   │ Text  │ Audio │ Video │ Actions        │ │
│  ├────────────────────────┼───────┼───────┼───────┼────────────────┤ │
│  │ ☐ Metamorphosis_UL1-6  │  ✅   │  —    │  —    │ (not marked)   │ │
│  │ ☑ Metamorphosis_UL7-10 │  ✅   │  ✅   │  ✅   │ Done           │ │
│  │ ☑ Metamorphosis_UL11   │  ✅   │  ✅   │  ❌   │ [Gen Video]    │ │
│  │ ☑ Metamorphosis_UL12   │  ✅   │  ❌   │  —    │ [Gen Audio]    │ │
│  │ ☑ Metamorphosis_UL13   │  ✅   │  ❌   │  —    │ [Gen Audio]    │ │
│  └────────────────────────┴───────┴───────┴───────┴────────────────┘ │
│                                                                      │
│  [Mark All] [Gen All Audio] [Gen All Video]                          │
│                                                                      │
│  Status: Generating audio for Metamorphosis_UL12...  ⏳              │
└──────────────────────────────────────────────────────────────────────┘
```

### GUI Behavior

- **Checkbox column** (☐/☑): Clicking toggles mark. Emits `av mark <stem>` or `av unmark <stem>`.
- **Status columns**: Filesystem-derived. ✅ = file exists. ❌ = marked but file missing. — = not applicable (e.g. video when no audio exists yet).
- **Action buttons**: Context-sensitive. Shows next logical action for each row.
  - If no audio → `[Gen Audio]` button (emits `av generate audio <stem>`)
  - If audio exists but no video → `[Gen Video]` button (emits `av generate video <stem>`)
  - If both exist → "Done" label
- **Batch buttons**: `[Mark All]` emits `av mark-all`. `[Gen All Audio]` emits `av generate audio all`. `[Gen All Video]` emits `av generate video all`.
- **Config button** (⚙): Opens a small inline settings panel or dialog for editing TTS/video config. Each change emits `av config tts <key> <value>`.
- **Open buttons** (📁): Emit `av open book-dir` / `av open audio-dir` etc. Opens system file explorer.
- **Progress**: Reuses the existing info bar / spinner pattern. When a generation subprocess is running, shows spinner + current stem name. No fine-grained progress bar initially.

### Refresh

The Media tab rescans the filesystem each time it is selected (or when `av scan` is run). Scanning is fast — it's just directory listing and file existence checks.

---

## Rust Implementation

### New Module: `src/services/av_producer.rs`

Handles manifest I/O, filesystem scanning, and subprocess spawning.

```rust
// Key types

pub struct AvManifest {
    pub tts: TtsConfig,
    pub video: VideoConfig,
    pub marked: Vec<String>,
}

pub struct TtsConfig {
    pub service: String,
    pub model: String,
    pub voices: Vec<String>,
    pub prompt_prefix: String,
    pub use_vertex_auth: bool,
    pub output_format: String,
    pub chunk_max_chars: u32,
    pub max_api_retries: u32,
    pub retry_delay: u32,
    pub concurrent_requests: u32,
}

pub struct VideoConfig {
    pub image_duration: u32,
    pub frame_rate: u32,
}

pub struct AvFileStatus {
    pub stem: String,
    pub marked: bool,
    pub has_text: bool,
    pub has_audio: bool,
    pub has_video: bool,
}

pub struct AvProducer {
    book_dir: PathBuf,
    manifest: AvManifest,
}
```

Key methods:
- `AvManifest::load(path) -> Result<Self>` — Read `_av_manifest.toml`
- `AvManifest::save(path) -> Result<()>` — Write `_av_manifest.toml`
- `AvManifest::default_for_book() -> Self` — Sensible defaults
- `AvProducer::new(book_dir) -> Result<Self>` — Initialize, load or create manifest
- `AvProducer::scan() -> Vec<AvFileStatus>` — Walk directory, derive status
- `AvProducer::generate_audio(stem, api_key) -> Result<Child>` — Spawn `book_to_audio.py`
- `AvProducer::generate_video(stem) -> Result<Child>` — Spawn `create_video.py`

### AppState Additions (in `src/app/state.rs`)

```rust
// All #[serde(skip)] — transient GUI/runtime state only
pub av_statuses: Vec<AvFileStatus>,
pub av_manifest_loaded: bool,
pub av_generating: Option<String>,          // stem currently being processed
pub av_generation_type: Option<String>,     // "audio" or "video"
pub av_queue: Vec<(String, String)>,        // remaining (stem, type) pairs for batch ops
pub show_av_config: bool,                   // toggle for config panel in Media tab
```

### Command Parsing (in `src/app/terminal.rs`)

New match arm for `"av"` prefix. All `av` subcommands parsed and dispatched as `AppCommand::Av*` variants.

### New AppCommand Variants (in `src/app/commands.rs`)

```rust
// AV Production
AvStatus,
AvScan,
AvMark { stems: Vec<String> },
AvUnmark { stems: Vec<String> },
AvMarkAll,
AvClearMarks,
AvGenerateAudio { target: AvTarget },   // AvTarget: Stem(String) | Next | All
AvGenerateVideo { target: AvTarget },
AvConfigShow,
AvConfigTts { key: String, value: String },
AvConfigVideo { key: String, value: String },
AvConfigVoices { voices: Vec<String> },
AvOpenDir { which: String },             // "book-dir", "audio-dir", "video-dir"
AvInit,
```

```rust
pub enum AvTarget {
    Stem(String),
    Next,
    All,
}
```

### Engine Handlers (in `src/app/engine.rs`)

Each `Av*` command dispatches to `AvProducer` methods. The engine:
1. Resolves the book directory from `output_dir` + `book_name`
2. Creates/loads the `AvProducer`
3. Executes the operation
4. Returns a formatted status string for the terminal

For generation commands, the engine spawns the Python subprocess and stores the `Child` handle. A poll mechanism (checked each frame in the GUI update loop, or via a channel) detects completion and advances to the next queued item if batch mode is active.

### GUI Component: `src/gui/components/media_view.rs`

Single `pub fn render(ui: &mut egui::Ui, state: &mut AppState)` function following existing patterns. All button clicks set `state.pending_terminal_command = Some("av ...")`.

### Top Bar Update: `src/gui/components/top_bar.rs`

Add a "Media" selectable tab. When clicked, switches the main view to the media panel.

---

## Python Script Modifications

### `book_to_audio.py`

Minimal changes needed:
- Accept `--api-key` argument (or read from `GOOGLE_API_KEY` env var — already supported)
- Accept `--output-dir` argument to specify where audio goes (instead of inferring from `content_project_dir`)
- Accept `--input-file` as a full path (not just filename)
- The Rust subprocess launcher passes the API key as an environment variable to the child process (secure — not on command line)

### `create_video.py`

Minimal changes needed:
- Accept explicit `--audio-file`, `--illustrations-dir`, `--output-dir` arguments
- Remove dependency on `content_project_dir` for path resolution — the Rust app provides all paths directly

These changes keep the scripts usable standalone while making them callable with explicit paths from the Rust app.

---

## API Key Flow

1. User sets Gemini API key via existing `set key gemini <value>` command (stored in OS keyring via `secrets.rs`)
2. When `av generate audio` is invoked, the engine retrieves the key from the keyring
3. The key is passed to `book_to_audio.py` as the `GOOGLE_API_KEY` environment variable on the child process (not as a command-line argument)
4. For Vertex AI auth (`use_vertex_auth = true`), the `GCLOUD_PROJECT_ID` is stored in `config.toml` (not sensitive) and passed similarly

---

## Implementation Sequence

### Phase 1: Foundation  ✅ COMPLETE
1. [x] `AvManifest` struct + TOML serialization/deserialization — `src/services/av_producer.rs`
2. [x] `AvProducer` with filesystem scan logic — `src/services/av_producer.rs`
3. [x] `AvFileStatus` derivation from directory listing — `src/services/av_producer.rs`
4. [x] Unit tests for manifest I/O and scanning — 9 tests, all passing

### Phase 2: Terminal Commands (read-only)  ✅ COMPLETE
5. [x] `av init` — create default manifest
6. [x] `av status` / `av scan` — display file status table
7. [x] `av mark` / `av unmark` / `av mark-all` / `av clear-marks`
8. [x] `av config show` / `av config tts` / `av config video` / `av config voices`
9. [x] `av open` commands (system file explorer) — uses `opener` crate
- [x] `AppCommand` enum extended with all Av* variants + `AvTarget` enum
- [x] Terminal parser with `av` subcommand routing + `av help`
- [x] Engine dispatch handlers for all read-only commands
- [x] Generate audio/video stubs (return "not yet implemented")
- [x] Help text updated with AV section

#### Files modified in Phase 2:
- `src/app/commands.rs` — Added `AvTarget` enum + 13 `Av*` command variants
- `src/app/terminal.rs` — Added `parse_av_command()` + `AvHelp` handler + help text
- `src/app/engine.rs` — Added `av_execute()`, `av_execute_mut()`, `resolve_av_book_dir()` + all dispatch arms
- `src/services/mod.rs` — Registered `av_producer` module
- `Cargo.toml` — Added `opener = "0.7"` dependency

### Phase 3: Audio Generation  ✅ COMPLETE
10. [x] Modify `book_to_audio.py` to accept explicit `--input-file` + `--output-dir` paths (backward compatible)
11. [x] `av generate audio <stem>` — subprocess spawning with full arg construction from manifest
12. [x] `av generate audio next` / `av generate audio all` — sequential generation with early-exit on error

#### Files modified in Phase 3:
- `book_to_audio.py` — Added `--input-file` (Path) and `--output-dir` (Path) args; `--input-filename` now optional; path resolution branches on which args are provided (explicit vs config.toml)
- `src/services/av_producer.rs` — Added `generate_audio()` method (builds Command with all TTS config, passes API key via env var), `next_stem_needing_audio()`, `all_stems_needing_audio()`, `next_stem_needing_video()`, `find_python()` free function
- `src/app/engine.rs` — Replaced `AvGenerateAudio` stub with full implementation: resolves book dir, retrieves Google API key from keyring, resolves stems from `AvTarget`, spawns subprocess, reports results; imported `AvTarget`

### Phase 4: Video Generation  ✅ COMPLETE
13. [x] Modify `create_video.py` to accept explicit `--audio-file`, `--illustrations-dir`, `--output-dir`, `--frame-rate`, `--image-duration` args (backward compatible with positional `book_name`)
14. [x] `av generate video <stem>` — subprocess spawning with full arg construction from manifest video config
15. [x] `av generate video next` / `av generate video all` — sequential generation with early-exit on error

#### Files modified in Phase 4:
- `create_video.py` — `book_name` now optional; added `--audio-file` (Path), `--illustrations-dir` (Path), `--output-dir` (Path), `--frame-rate`, `--image-duration` args; explicit path mode processes a single audio file; legacy mode preserved
- `src/services/av_producer.rs` — Added `generate_video()` method (builds Command with video config, validates audio + illustrations), `all_stems_needing_video()`
- `src/app/engine.rs` — Replaced `AvGenerateVideo` stub with full implementation: resolves book dir, pre-checks illustrations, resolves stems from `AvTarget`, spawns subprocess, reports results

### Phase 5: GUI  ✅ COMPLETE
16. [x] `media_view.rs` component — status table rendering with `egui::Grid`, config summary, error states
17. [x] Top bar "Media" tab integration — toggle `show_media_tab` flag, selectable label
18. [x] Mark/unmark checkboxes — per-row checkboxes emit `av mark`/`av unmark` commands
19. [x] Generate buttons (single + batch) — per-row context-sensitive "Gen Audio"/"Gen Video" + batch "Gen All Audio"/"Gen All Video"
20. [x] Config panel — compact summary line (service/model/voices/chunk + video fps/duration) with "Config..." button linking to `av config show`
21. [x] Progress indicator — reuses existing info bar spinner pattern; illustrations count shown in summary bar

#### Files modified in Phase 5:
- `src/gui/components/media_view.rs` — NEW: Full Media tab component with `render()`, status table (`egui::Grid` with striped rows), mark/unmark checkboxes, per-row action buttons, batch buttons, config summary, directory open buttons, `load_statuses()` helper
- `src/gui/components/mod.rs` — Registered `media_view` module
- `src/gui/components/top_bar.rs` — Added "Media" selectable tab that toggles `show_media_tab`
- `src/gui/app.rs` — Central panel routes to `media_view::render()` when `show_media_tab` is true
- `src/app/state.rs` — Added `show_media_tab: bool` field (serde skip, default false)

### Phase 6: Polish ✅ COMPLETE
22. [x] Error handling for missing illustrations, missing API keys, ffmpeg not found
23. [x] Terminal help text for `av` commands
24. [x] `av` command in copilot.ps1 relay (works automatically — relay passes raw text)

**Changes:**
- `src/services/av_producer.rs` — Added `validate_python()` and `validate_ffmpeg()` pre-flight checks; both `generate_audio()` and `generate_video()` now verify Python before spawning, `generate_video()` also verifies ffmpeg
- `src/app/engine.rs` — Added empty API key guard before audio generation
- `src/app/terminal.rs` — Enhanced `av help` text with prerequisites section (Python, API key, ffmpeg, illustrations)

### Phase A: Streaming Output + Cancel ✅ COMPLETE
25. [x] `AvJobState` shared struct (`Arc<Mutex<>>`) with output_lines, cancel_requested, finished, child_pid — `src/app/state.rs`
26. [x] `spawn_audio()` / `spawn_video()` methods returning `Child` with piped stdout/stderr — `src/services/av_producer.rs`
27. [x] Background thread reads subprocess stdout/stderr line-by-line into shared buffer — `av_job_reader()` in `src/app/engine.rs`
28. [x] `AvGenerateAudio` / `AvGenerateVideo` dispatch spawns thread + returns immediately — `src/app/engine.rs`
29. [x] GUI polls `AvJobState` each frame, drains new lines to terminal_history — `src/gui/app.rs`
30. [x] `av cancel` / `av stop` command + Cancel button in Media tab — kills process tree via `taskkill /PID /T /F`
31. [x] Job status bar with spinner + label + disabled Gen buttons while job running — `src/gui/components/media_view.rs`

#### Files modified in Phase A:
- `src/app/state.rs` — Added `AvJobState` struct, `av_job: Option<Arc<Mutex<AvJobState>>>` field
- `src/app/commands.rs` — Added `AvCancel` variant
- `src/app/terminal.rs` — Added `av cancel`/`av stop` parsing, updated help text
- `src/app/engine.rs` — Rewrote `AvGenerateAudio`/`AvGenerateVideo` to spawn background threads, added `AvCancel` handler, added `av_job_reader()` free function
- `src/services/av_producer.rs` — Added `spawn_audio()` and `spawn_video()` methods
- `src/gui/app.rs` — Added `av_job_lines_seen` field, AV job poll loop draining lines to terminal_history
- `src/gui/components/media_view.rs` — Job status bar with spinner + Cancel button, disabled Gen buttons during job

### Phase B: Chunk Directory Structure + Python Script Updates ✅ COMPLETE
32. [x] Modify `book_to_audio.py`: accept `--chunks-dir` argument, write text chunks as `.txt` files alongside audio `.wav` chunks
33. [x] Modify `book_to_audio.py`: on re-run, detect gaps (missing `.wav` where `.txt` exists, or `.wav.bad` files) and regenerate only those
34. [x] Modify `book_to_audio.py`: add `--no-concat` flag to skip final concatenation (let Rust side control rebuild)
35. [x] `AvProducer`: scan `audio/chunks/<stem>/` directory — return `Vec<ChunkStatus>` with index, has_text, has_audio, is_rejected
36. [x] `AvProducer`: pass `--chunks-dir` to `spawn_audio()` command args, pointing to `audio/chunks/<stem>/`
37. [x] Unit tests for chunk scanning and status derivation

#### Files modified in Phase B:
- `book_to_audio.py` — Added `--chunks-dir` (Path) and `--no-concat` (flag) args; when `--chunks-dir` is provided, auto-enables gap-detection mode (existing chunks preserved, only gaps regenerated); `--no-concat` skips final concatenation
- `src/services/av_producer.rs` — Added `ChunkStatus` struct, `parse_chunk_index()` helper, `chunks_dir()` and `scan_chunks()` methods on `AvProducer`; both `generate_audio()` and `spawn_audio()` now pass `--chunks-dir` pointing to `audio/chunks/<stem>/`; 3 new unit tests (181 total)

### Phase C: GUI Chunk Detail Panel + Reject/Status ✅ COMPLETE
38. [x] Right-side chunk detail panel in `media_view.rs` — opens when a stem row is clicked in the status table
39. [x] Chunk list display: each row shows chunk index, text status, audio status (green=good, red=rejected, grey=missing), action button
40. [x] "Reject" button per chunk — emits `av reject chunk <stem> <index>` which renames `.wav` → `.wav.bad`
41. [x] "Restore" button for `.wav.bad` chunks — emits `av restore chunk <stem> <index>` which renames back
42. [x] Stale final audio indicator — orange ⚠ in the Audio column when final `.wav` exists but chunks are rejected/missing
43. [x] `av reject chunk <stem> <index>` / `av restore chunk <stem> <index>` terminal commands
44. [x] `av chunks <stem>` terminal command — list all chunks with status table

#### Files modified in Phase C:
- `src/app/commands.rs` — Added `AvRejectChunk`, `AvRestoreChunk`, `AvChunkStatus` variants
- `src/app/terminal.rs` — Added parsing for `av reject chunk`, `av restore chunk`, `av chunks`; updated both help texts
- `src/app/engine.rs` — Added dispatch handlers: `AvChunkStatus` (formatted table), `AvRejectChunk` (rename to `.bad`), `AvRestoreChunk` (rename back)
- `src/app/state.rs` — Added `av_selected_stem: Option<String>` field for chunk panel selection
- `src/gui/components/media_view.rs` — Split into `render_main_panel` + `render_chunk_panel`; stem names are clickable `selectable_label`; chunk panel shows index/text/audio/action grid; stale audio ⚠ indicator on stem rows; `ChunkDetailData` struct + `load_chunk_data()` helper

### Phase D: `av rebuild audio` Command ✅ COMPLETE
45. [x] `av rebuild audio <stem>` terminal command — concatenates all good chunks (non-`.bad`) into final `audio/<stem>.wav`
46. [x] Rust-side implementation: `rebuild_audio()` method invokes `book_to_audio.py --concat-only --chunks-dir <dir>` (blocking, fast)
47. [x] GUI "Rebuild" button in chunk panel — appears inline with stale audio ⚠ warning when chunks have been modified
48. [x] `--concat-only` flag added to `book_to_audio.py` — skips TTS/API key setup entirely, concatenates good `.wav` chunks via pydub

#### Files modified in Phase D:
- `book_to_audio.py` — Added `--concat-only` flag; when set, skips API client setup and TTS generation, scans chunks dir for good `.wav` files (excluding `.wav.bad`), sorts by index, concatenates via pydub, exports final audio
- `src/app/commands.rs` — Added `AvRebuildAudio { stem: String }` variant
- `src/app/terminal.rs` — Added `"rebuild"` match arm parsing `av rebuild audio <stem>`; updated both help text sections
- `src/app/engine.rs` — Added `AvRebuildAudio` dispatch: resolves book dir, creates producer, calls `rebuild_audio()`
- `src/services/av_producer.rs` — Added `rebuild_audio(&self, stem, project_root)` method: validates chunks exist, counts good chunks, spawns blocking `book_to_audio.py --concat-only`, verifies output file
- `src/gui/components/media_view.rs` — Added "Rebuild" button next to stale audio warning in chunk panel, emits `av rebuild audio <stem>`

---

## Audio Chunk Workflow

The production workflow for audio generation involves iterative review and correction:

1. **Initial generation**: `av generate audio <stem>` runs `book_to_audio.py` which splits text into TTS-sized chunks, writes `chunk_NNN.txt` + `chunk_NNN.wav` to `audio/chunks/<stem>/`, then concatenates all chunks into the final `audio/<stem>.wav`.

2. **Review**: User listens to the full audio. Inevitably ~1 in 10 chunks has a glitch (silence appended, garbled speech, wrong pacing). User identifies bad chunks by timestamp or chunk index.

3. **Reject bad chunks**: In the GUI chunk panel or via `av reject chunk <stem> <index>`, the bad `.wav` is renamed to `.wav.bad`. The final audio is flagged as stale.

4. **Regenerate gaps**: `av generate audio <stem>` re-runs the Python script. It detects gaps (missing `.wav` where `.txt` exists) and regenerates only those chunks. Existing good chunks are preserved.

5. **Re-review**: User reviews the newly generated chunks (marked distinctly in the GUI). If still bad, reject and repeat step 4.

6. **Rebuild final**: Once all chunks are good, `av rebuild audio <stem>` concatenates them into the final `.wav`.

This replaces the previous manual workflow of spelunking through directories, deleting files, and re-running scripts from the command line.

---

## Out of Scope (Future Work)

- **Image generation** — illustrations are created externally for now
- **Embedded audio/video player** — user opens files in external player
- **MP3/FLAC output** — WAV only for now; extensible via `output_format` config
- **Concurrent file generation** — sequential only; `concurrent_requests` option preserved for future use
- **YouTube upload integration** — manual upload for now
