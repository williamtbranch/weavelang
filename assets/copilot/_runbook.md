# WeaveLang Copilot Runbook

Operational reference for the autonomous copilot agent. This file is read-only
during a session — update it between sessions when procedures change.

---

## 1. What is WeaveLang

WeaveLang is a desktop application for producing bilingual "woven" texts and
audio-visual content from them. The core workflow:

1. **Import** a source-language text (e.g. an English book from Project Gutenberg).
2. **Segment** it into sentences. Each sentence gets multiple tiers:
   - *Source* — the original sentence.
   - *Bas B / Bas T* — word-level base-language / target-language token pairs (mappings).
   - *Moderate* — a mid-difficulty target-language paraphrase.
   - *Advanced* — a fully natural target-language translation.
3. **Generate** tiers using LLM stages (Gemini / Claude). Each stage has a model
   alias, a prompt, and validation rules. Generation results are reviewed,
   approved, or re-run per sentence.
4. **Calibrate** difficulty levels using an AVD (Automated Vocabulary Difficulty)
   scoring system. Calibration produces a level map that controls how much
   target language appears at each numbered level (UL0 = 100% source, higher
   levels mix in more target language).
5. **Weave** — `generate_weave` merges the approved tiers into output text files
   at specified difficulty levels. A Design Rule Check (DRC) ensures data
   integrity before weaving.
6. **AV production** — woven text files become the basis for audio (TTS),
   illustrations (image generation), and video (combining audio + images).
7. **YouTube upload** — finished videos are uploaded with templated metadata.

Projects are organised into **books** containing **chapters**. Each chapter maps
to a range of sentences. AV production operates per-chapter, per-stem (a stem
is the filename base for a woven text file, e.g. `grimms_The_Golden_Bird_UL14`).

---

## 2. Communication Protocol

The copilot agent is embedded in the WeaveLang GUI. It uses the same LLM
infrastructure as generation stages (Gemini / Claude via model aliases).

### Talking to the Copilot

Type `$` followed by a message in the WeaveLang terminal:
```
$ what is the status of AV production?
$ produce chapters 3 and 4 today
$ please check if there is anything to do from the last session
```

The copilot's responses appear as `[copilot]` lines in the terminal.
The copilot can execute terminal commands on your behalf.

### Configuration

Set the copilot model in **Preferences → LLM Settings** (Co-pilot Agent section),
or via terminal:
```
config set copilot.model sonnet          # use model alias from [models]
config set copilot.model none            # disable copilot
config set copilot.max_turns 50          # safety cap per session
```

### Auto-start

When a workspace opens, the copilot checks `copilot/_goal.toml`.
If it contains actionable content (non-empty chapters + at least one step enabled),
the copilot begins executing the plan automatically.
If the goal is empty, it reports "nothing to do" and waits for `$` messages.

### Session Persistence

Conversation history is **automatically saved** to `copilot/_session.json`
and restored when the workspace reopens. The copilot picks up where it left off.

To start a completely fresh session:
```
copilot reset
```

### Journal

The copilot can log progress to `copilot/_journal.md` (timestamped, append-only).
This survives across restarts and is included in the copilot's workspace context.
```
copilot journal Completed audio generation for Old_Sultan_UL19
copilot journal Error: video generation failed — missing illustrations
```

### Self-Help Commands

If you are unsure about available commands or their syntax, ask the app:

```
help                                   # full command list
av help                                # AV production command list
```

These always reflect the running version of the app, so prefer them over this
runbook if anything seems out of date.

---

## 3. Pipeline Stages (in order)

The full production pipeline for a chapter, from raw data to YouTube:

### 3.1 Project & Chapter Setup

```
open workspace "<workspace_path>"
load project "<project_file>"          # e.g. library\grimms.wvl
list chapters                          # list all chapters with ranges and validity
select chapter "<chapter_name>"        # e.g. "The Travelling Musicians"
new chapter "<name>" <start> <end>     # define a chapter (1-based sentence range)
delete chapter "<name>"                # remove a chapter definition
list nav [N]                           # list sentences (N = page, 1-based)
list nav --around <N>                  # show context around sentence N (5 before/after)
list headings                          # scan for chapter/section headings (auto-detect)
search "<text>"                        # find all sentences containing text (case-insensitive)
select sentence <N>                    # select sentence by number (1-based)
report sentences incomplete [N]         # list sentences not ready (N = limit, omit for all)
report sentences complete [N]           # list weave-ready sentences (N = limit)
report sentence <N>                    # detailed status for sentence N
accept map <start> <end>              # bulk accept stale mappings in range (requires tier selected)
```

### 3.2 Dependency Graph

**CRITICAL**: Every step depends on the previous step completing successfully.
Steps that launch LLM or subprocess jobs are **asynchronous** — you MUST
run `watch_job` after each one to wait for completion before proceeding.

```
1. LLM Generation (tiers)        ← source text must exist
   ↓  (6 stages, each async, run in order)
2. DRC passes                    ← all tiers Valid + mappings complete
   ↓
3. Calibrate                     ← DRC must pass
   ↓
4. generate_weave <level>        ← calibration + DRC done, produces .txt files
   ↓
5. av init + av mark             ← .txt files must exist
   ↓
6. av generate audio <stem>      ← .txt file must exist (async, wait)
   ↓
7. av generate prompts           ← needs UL0 text file (async, wait)
   ↓
8. av generate illustrations     ← needs prompts (async, wait)
   ↓
9. av generate video <stem>      ← needs audio + at least 1 illustration (async, wait)
   ↓
10. av youtube upload <stem>     ← needs video file + auth
```

### 3.3 LLM Tier Generation

Each sentence needs 6 LLM generation stages run **in order** before it passes DRC.
Use `run generate <StageName> <start> <end>` where start/end are 1-based sentence numbers.

| Order | Stage Name | Input Tier | Output Tier | Notes |
|-------|-----------|-----------|------------|-------|
| 1 | `GenerateAdvancedTarget` | base (source) | advanced_target | Full natural translation |
| 2 | `GenerateModerateTarget` | advanced_target | moderate_target | Mid-difficulty paraphrase |
| 3 | `GenerateBasicBase` | base (source) | basic_base | Simplified source |
| 4 | `GenerateBasicTarget` | basic_base | basic_target | Basic-level translation |
| 5 | `GeneratePhraseMap` | basic_base | fwd_mapping | Word-level mapping B→T (auto-queued after stage 3) |
| 6 | `GenerateInversePhraseMap` | basic_target | inv_mapping | Word-level mapping T→B (auto-queued after stage 4) |

**Important**: Stages 5 and 6 are automatically queued when stages 3 and 4 complete.
You only need to explicitly run stages 1-4. Always `watch_job` between stages.

Example sequence for sentences 301-326:
```
run generate GenerateAdvancedTarget 301 326
watch_job
run generate GenerateModerateTarget 301 326
watch_job
run generate GenerateBasicBase 301 326
watch_job
run generate GenerateBasicTarget 301 326
watch_job
```

After all stages complete, verify with:
```
report sentences incomplete 5           # check first 5 incomplete (should be none in range)
drc                                     # run full Design Rule Check
```

### 3.4 Calibration & Weaving

**Before calibrating:** Always run `calibrate info` first to check whether
calibration is necessary. If the level map was generated from ≥ 800 sentences
the calibration is stable and should NOT be re-run (it takes ~30 minutes and
shifts level boundaries, causing minor inconsistencies with already-finished
weave/audio files). If fewer than 800 sentences were used, finish more
sentences first, then recalibrate.

**Strategy for new books:** Focus on completing the first ~800 sentences
(all LLM stages + DRC pass) before running the first calibration. This
avoids wasting time on provisional calibrations that will shift later.

```
calibrate info                         # check sentence count & stability of current map
calibrate [max_level]                  # calibrate AVD levels for the book
generate_weave <level> [--force]       # produce woven text files
```

**Level specifiers:**

| Arg | Meaning | Output suffix |
|-----|---------|---------------|
| `0` | Acclimatization (100% base language) | `UL0` |
| `<N>` | Numeric level from calibrated level map | `UL<N>` or `UL<N>-<M>` |
| `b` | Basic-only (max basic, no mod/adv) | `ULb<N>` |
| `m` | Moderate (basic + moderate) | `ULm<N>` |
| `a` | Advanced (all tiers) | `ULa<N>` |
| `i` | Interlinear triplet | `ULi<N>` |
| `all` | Every level in the level map | multiple files |

**Prerequisites**: All sentences audited, output_dir set, level_map imported.
`--force` bypasses the DRC check.

### 3.5 AV Init & Marking

```
av init                                # create _av_manifest.toml (if needed)
av status                              # show stem table with text/audio/video status
av mark <stem> [stem2 ...]             # mark stems for production
av mark-all                            # mark every woven text file
```

### 3.6 Audio Generation (TTS)

```
av generate audio <stem>               # one file
av generate audio next                 # next file missing audio
av generate audio all                  # all marked files missing audio
```

This spawns `book_to_audio.py` as a subprocess. Audio goes to `audio/<stem>.wav`.
Check progress with `job_status` or `av log`.

**Chunk management** (if a chunk sounds wrong):
```
av chunks <stem>                       # list chunks and status
av reject chunk <stem> <N>             # rename .wav → .wav.bad
av restore chunk <stem> <N>            # undo rejection
av rebuild audio <stem>                # re-concatenate good chunks
```

### 3.7 Illustration Generation

```
av generate prompts                    # LLM writes illustration prompts
av generate illustrations              # image model creates PNGs
```

Illustrations go to `illustrations/`. At least one image required for video.

### 3.8 Video Generation

```
av generate video <stem>               # one file (requires audio + illustrations)
av generate video next                 # next eligible
av generate video all                  # all eligible
```

Video goes to `video/<stem>.mp4`.

### 3.9 YouTube Upload

```
av youtube auth                        # OAuth consent (one-time, opens browser)
av youtube upload <stem>               # upload one video
av youtube upload next                 # next video not yet uploaded
av youtube upload all                  # all videos with no upload record
```

Upload tracking is in `_youtube.toml [uploads]` section (stem = video_id).
The `--dry-run` flag on the Python script shows resolved metadata without uploading.

### 3.10 Status & Monitoring

```
av status                              # table of all stems: text/audio/video status
av log [N]                             # last N lines of AV subprocess output
job_status                             # LLM/AV job state (IDLE, RUNNING, DONE, ERROR)
```

---

## 4. Directory Layout

```
{workspace}/
├── config.toml                        # workspace config
├── _chapters.toml                     # chapter list
├── library/
│   └── <book>.wvl                     # project file
├── weave_out/
│   └── <book>/                        # book output
│       ├── _av_manifest.toml          # AV production config + marked files
│       ├── <chapter_name>/
│       │   ├── <stem>.txt             # woven text (source of truth)
│       │   ├── audio/
│       │   │   ├── <stem>.wav         # final audio
│       │   │   └── chunks/<stem>/     # per-chunk txt + wav
│       │   ├── video/
│       │   │   └── <stem>.mp4         # final video
│       │   ├── illustrations/
│       │   │   ├── _prompts.toml      # generated prompts
│       │   │   ├── 001.png ...        # generated images
│       │   └── _youtube.toml          # upload config + tracking
│       └── whole_book/                # whole-book output (if used)
└── copilot/
    ├── _runbook.md                    # this file
    ├── _goal.toml                     # user-defined production goals
    ├── _plan.toml                     # agent task list
    └── _journal.md                    # append-only execution log
```

---

## 5. Configuration Files

### _av_manifest.toml

Located at `weave_out/<book>/` level. Controls TTS, video, and illustration settings.

Key sections:
- `[tts]` — service, model, voices, prompt_prefix, chunk_max_chars, retries
- `[video]` — image_duration, frame_rate
- `[illustrations]` — style_prefix, prompt_model, image_model, image_size
- `[files]` — `marked = [...]` list of stems for production

### _youtube.toml

Located per-chapter. Controls upload metadata and tracks uploaded videos.

Key sections:
- `[metadata]` — title_template, description_template, tags, category_id, privacy
- `[auth]` — client_secret_file path
- `[variables]` — template variables (language, etc.)
- `[uploads]` — stem = "video_id" records (prevents duplicates)

Template variables auto-extracted from stem: `{chapter_name}`, `{level}`, `{book_name}`, `{level_tag}`, `{stem}`.

---

## 6. Error Patterns & Recovery

### GUI not responding
- **Symptom**: `ping` times out or connection refused.
- **Recovery**: The GUI must be started manually by the user. Log the failure and stop.

### TTS API errors
- **Symptom**: `av log` shows API errors (rate limit, auth failure, model unavailable).
- **Recovery**: Wait `retry_delay` seconds (from manifest). Max `max_api_retries` attempts.
  After exhausting retries, log the stem as failed and move to next.

### Video generation fails
- **Symptom**: Missing audio file or no illustrations.
- **Recovery**: Check prerequisites (`av status`). If audio missing, generate it first.
  If no illustrations, log and skip — illustrations must exist.

### YouTube upload fails
- **Symptom**: HTTP 400/403/5xx from YouTube API.
- **Recovery**:
  - 403 "quota exceeded" → stop all uploads for today, log daily limit reached.
  - 403 "API not enabled" → log, require user intervention.
  - 400 "upload limit" → may be platform outage, retry once after 5 min, then stop.
  - Auth errors → token may be expired. Try `av youtube auth` (requires browser — stop if unattended).

### Chunk quality issues
- **Symptom**: Audio chunk sounds wrong (detected by user, not agent).
- **Note**: The agent cannot assess TTS quality. Chunk rejection is a user-initiated action.

---

## 7. Safety Rules

1. **Never delete files**. Mark, skip, or log — never remove user data.
2. **Never force-push or overwrite** without `--force` being explicitly in the goal.
3. **Stop on auth failures**. OAuth requires a browser — the agent cannot complete auth unattended.
4. **Respect daily upload limits**. YouTube has daily upload quotas. If an upload fails with quota/limit errors, stop all uploads for the session.
5. **Log everything**. Every command executed and its result goes in `_journal.md`.
6. **Fail gracefully**. If a stem fails, log it and continue with the next. Don't abandon the whole plan.
7. **Don't guess stems**. Only operate on stems listed in `_goal.toml` or derived from explicit level specifications.
