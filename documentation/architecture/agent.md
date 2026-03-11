# Co-Pilot Agent Mode

Instructions for an AI agent to drive the WeaveLang GUI from VS Code.

> **Related docs in this directory:**
> - [Terminal_Commands.md](Terminal_Commands.md) — Full command reference (read this first)
> - [App_Overview.md](App_Overview.md) — What the app does, tier structure, and dependencies

---

## 1. Check if the GUI is Already Running

```powershell
.\copilot.ps1 ping
```

If you get a JSON response with `"success": true`, the server is live — skip to **§3**.

If it fails (connection refused), either:
- The GUI is not running, or
- The GUI was built without the copilot relay server.

---

## 2. Start the GUI

```powershell
# Build (if needed — only required after code changes)
cargo build --bin weavelang_rust_gui

# Launch (detached, so the terminal is free)
Start-Process .\target\debug\weavelang_rust_gui.exe

# Wait for startup, then verify
Start-Sleep 3
.\copilot.ps1 ping
```

The copilot relay server starts automatically on port **3030** (configurable via `copilot_server_port` in `config.toml`). A green 🤖 indicator appears in the GUI info bar when it's active.

To **stop** the GUI from the agent:

```powershell
Get-Process weavelang_rust_gui -ErrorAction SilentlyContinue | Stop-Process -Force
```

---

## 3. The Wrapper Script

All copilot communication goes through `copilot.ps1` at the project root:

| Command | Purpose |
|---------|---------|
| `.\copilot.ps1 ping` | Health check |
| `.\copilot.ps1 state` | Full document state as JSON |
| `.\copilot.ps1 sentence <N>` | Single sentence detail (1-based) |
| `.\copilot.ps1 cmd "<command>"` | Execute a terminal command |
| `.\copilot.ps1 shutdown` | Shut down the relay server |

### Sending Terminal Commands

```powershell
.\copilot.ps1 cmd "select sentence 5"
.\copilot.ps1 cmd "show detail"
.\copilot.ps1 cmd "select tier bas_b"
.\copilot.ps1 cmd "edit The Grimm Story Books"
```

Commands are the same as those typed in the GUI terminal. See [Terminal_Commands.md](Terminal_Commands.md) for the full list.

### Reading Structured Data

```powershell
# Get full document state (sentence count, languages, per-sentence tier summary)
.\copilot.ps1 state

# Get one sentence (1-based numbering, matches `select sentence N`)
.\copilot.ps1 sentence 5
```

These return JSON with `success`, `message`, and `data` fields.

---

## 4. Visibility — The Tee

Every action the agent takes is logged in the GUI terminal panel so the user can watch:

- **`[copilot]> select sentence 5`** — Terminal commands (blue in GUI)
- **`[copilot] query sentence 5 → S5 "Chapter 1:"`** — API queries (green in GUI)
- **`[copilot] ping`** — Health checks

The user sees exactly what the agent does, in real time.

---

## 5. Typical Workflows

### Explore a Loaded Book

```powershell
.\copilot.ps1 state                                 # How many sentences? What languages?
.\copilot.ps1 cmd "weave status"                    # Ready for weave output?
.\copilot.ps1 cmd "report sentences incomplete"     # Which sentences have issues?
.\copilot.ps1 cmd "report sentence 7"               # Drill into one sentence
.\copilot.ps1 sentence 7                            # Get full JSON for sentence 7
```

### Edit a Tier

```powershell
.\copilot.ps1 cmd "select sentence 4"
.\copilot.ps1 cmd "select tier bas_b"               # Tier aliases: bas_b, bas_t, adv, mod, base
.\copilot.ps1 cmd "edit The Grimm Story Books"      # Replace the tier's full text
.\copilot.ps1 cmd "validate 4 bas_b"                # Re-lemmatize + mark Valid
.\copilot.ps1 cmd "select tier bas_t"
.\copilot.ps1 cmd "accept map"                      # Re-validate the mapping
.\copilot.ps1 cmd "save project"
```

### Batch-Accept Stale Tiers

When `basic_target` is Stale on many sentences (common after generation):

```powershell
for ($i = 1; $i -le 23; $i++) {
    .\copilot.ps1 cmd "select sentence $i"
    .\copilot.ps1 cmd "select tier bas_t"
    .\copilot.ps1 cmd "accept map"
}
.\copilot.ps1 cmd "weave status"
.\copilot.ps1 cmd "save project"
```

### Generate Weave Output

```powershell
.\copilot.ps1 cmd "set output_dir e:\path\to\weave_output"
.\copilot.ps1 cmd "generate_weave all"              # All levels
.\copilot.ps1 cmd "generate_weave 11"               # Single level
```

### Rebuild After Code Changes

```powershell
Get-Process weavelang_rust_gui -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
cargo build --bin weavelang_rust_gui
Start-Process .\target\debug\weavelang_rust_gui.exe
Start-Sleep 3
.\copilot.ps1 ping
```

---

## 6. Important Notes

- **Sentence numbers are 1-based** everywhere (terminal commands, API, wrapper script).
- **Tier aliases**: `base`/`source`, `adv`, `mod`, `bas_t`/`basic_t`, `bas_b`/`basic_b`.
- **Editing marks a tier Dirty**. After editing, run `validate N <tier>` to re-lemmatize and mark it Valid. If the edit was to a basic tier, also `accept map` to re-validate the mapping.
- **Editing propagates staleness** downstream. E.g., editing `base` makes `advanced_target` and `basic_base` Stale.
- **`weave status`** is the quickest readiness check — it tells you if all sentences are complete.
- **`save project`** persists to the last-loaded `.wvl` file. Always save after edits.
- **The GUI must be running** for the copilot to work. If the exe is locked for rebuild, stop the GUI first.
- **Config survives restart** (loaded from `config.toml`), but runtime state like `output_dir` does not — re-set it after relaunch.

---

## 7. API Endpoints (Advanced)

For direct HTTP access without the wrapper:

| Method | Endpoint | Body | Returns |
|--------|----------|------|---------|
| GET | `/api/v1/ping` | — | JSON health check |
| GET | `/api/v1/state` | — | Full document state JSON |
| GET | `/api/v1/state/sentence/<N>` | — | Single sentence JSON (1-based) |
| POST | `/api/v1/terminal` | `text/plain` command | Terminal output as text |
| POST | `/api/v1/shutdown` | — | Shutdown acknowledgment |

Base URL: `http://127.0.0.1:3030` (default port).