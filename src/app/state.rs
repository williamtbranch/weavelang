// src/gui/state.rs

use crate::domain::sentence::Sentence;
use crate::services::llm_client::LlmService;
use crate::services::llm_logger::LlmLogger;
use crate::services::prompt_manager::PromptManager;
use crate::services::python_bridge::BridgeService;
use crate::simulation::numerical_types::VLevelRecipe;
use crate::types::json_types::JsonCurriculumMap;
use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Chapter — a named range of sentences for incremental publishing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub name: String,
    pub start: usize, // 1-based inclusive
    pub end: usize,   // 1-based inclusive
}

// ---------------------------------------------------------------------------
// AV Job — shared state for background audio/video generation
// ---------------------------------------------------------------------------

/// Shared state for a background AV generation job.
/// Owned by both the spawned thread and the GUI poll loop.
#[derive(Debug)]
pub struct AvJobState {
    /// Lines of output from the subprocess (appended by the worker thread).
    pub output_lines: Vec<String>,
    /// Set to true to request cancellation.
    pub cancel_requested: bool,
    /// Set to true when the job thread has finished (success or failure).
    pub finished: bool,
    /// Final result message (set on finish).
    pub result_message: Option<String>,
    /// PID of the running child process (for kill on cancel).
    pub child_pid: Option<u32>,
    /// Description shown in the UI (e.g. "Generating audio: Metamorphosis_UL26")
    pub label: String,
}

// ... (Enums TierView, DetailView, SimulationMode remain unchanged) ...
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TierView {
    Base,
    AdvancedTarget,
    ModerateTarget,
    BasicTarget,
    BasicBase,
    Simulation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetailView {
    Tier(TierView),
    Token(TierView),
    MappingDiglot,
    MappingInverse,
    ProperNounLemmas,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SimulationMode {
    Calibrated,
    #[default]
    Manual,
}

#[derive(Serialize, Deserialize)]
pub struct AppState {
    /// Project schema version. Bumped whenever a non-backward-compatible
    /// field semantics change. See `documentation/Wlemma_Migration_Plan.md`.
    /// `1` = pre-wlemma. `2` = wlemmas populated everywhere.
    /// Old `.wvl` files (no field) deserialize to `0` and are upgraded on load.
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub document: Vec<Sentence>,
    #[serde(default)]
    pub book_map: Option<HashMap<String, JsonCurriculumMap>>,
    #[serde(default)]
    pub calibration_sentence_count: Option<usize>,
    #[serde(default)]
    pub book_name: String,

    // Frontier (context diffusion) settings persisted in .wvl
    #[serde(default)]
    pub frontier_enabled: bool,
    #[serde(default = "default_frontier_target_pct")]
    pub frontier_target_pct: f32,
    #[serde(default = "default_frontier_seed")]
    pub frontier_seed: u64,
    #[serde(default)]
    pub frontier_test_mode: bool,
    #[serde(default = "default_frontier_familiar_lemma_exclude_count")]
    pub frontier_familiar_lemma_exclude_count: usize,

    // --- NEW FIELD: Project Languages (Base, Target) ---
    #[serde(default = "default_languages")]
    pub project_languages: (String, String),

    // --- Friendly Lemmas / Simple Mode / Embedded Level Map (Phase A) ---
    /// Lemmas marked as "protective" by the author. When the lemmatizer
    /// finalises a `MappingEntry.target_lemmas`, if any friendly lemma
    /// appears in the candidate set, all non-friendly lemmas are dropped
    /// and the lowest-rank friendly lemma becomes the sole gate.
    #[serde(default)]
    pub friendly_lemmas: Vec<String>,
    /// Master toggle for the friendly-lemma shielding pass. Default on.
    #[serde(default = "default_true")]
    pub friendly_shielding_enabled: bool,
    /// When on, only the basic branch (basic_base / basic_target) is
    /// built, validated, and woven. Advanced + moderate tiers are
    /// skipped end-to-end.
    #[serde(default)]
    pub simple_mode: bool,
    /// When on, `simple_triple` ("3 non-diglot levels + 1 diglot") output
    /// mode is active: `basic_base` is turned off and only the single
    /// `basic_target` tier is woven (with a higher diglot frontier mix);
    /// the advanced + moderate tiers are emitted verbatim (no weaving).
    /// See `documentation/Simple_Triple_Mode_Plan.md`. Generation/weave
    /// consumption of this flag is staged; the toggle is wired now.
    #[serde(default)]
    pub simple_triple: bool,
    /// When on, chapter lesson text gets an LLM realignment pass before
    /// TTS so the emitted Spanish stays faithful to the original lesson.
    #[serde(default)]
    pub lesson_realign_enabled: bool,
    /// Assertion that the source file is already authored at the basic
    /// (simple-reader) level. When true (and `simple_mode` is on), the
    /// in-source-language basic-tier generation stage bypasses the LLM
    /// and copies `base` → that tier verbatim. The cross-language basic
    /// tier is still generated normally. Set by
    /// `%%META source_is_basic: on%%` at import.
    #[serde(default)]
    pub source_is_basic: bool,
    /// True when the loaded level map came from `%%META lm_entry%%`
    /// directives in the source text rather than `calibrate` or
    /// `import level_map`. Blocks `calibrate` from overwriting it.
    #[serde(default)]
    pub level_map_embedded: bool,

    #[serde(default)]
    pub selected_sentence_idx: usize,
    // Optional contiguous selection range in the navigator (inclusive start,end).
    // If None, only `selected_sentence_idx` is selected.
    #[serde(default)]
    pub selected_range: Option<(usize, usize)>,
    #[serde(default = "default_left_view")]
    pub left_view: TierView,
    #[serde(default = "default_right_view")]
    pub right_view: DetailView,

    // Simulation Settings
    #[serde(default)]
    pub sim_mode: SimulationMode,
    #[serde(default = "default_sim_user_level")]
    pub sim_user_level: u32,
    #[serde(default = "default_sim_manual_recipe")]
    pub sim_manual_recipe: VLevelRecipe,

    // Services
    #[serde(skip)]
    pub tool_root_dir: Option<PathBuf>,
    #[serde(skip)]
    pub bridge: Option<BridgeService>,
    #[serde(skip)]
    pub llm: Option<LlmService>,
    #[serde(skip)]
    pub prompts: Option<PromptManager>,
    #[serde(skip)]
    pub logger: Option<LlmLogger>,
    #[serde(skip)]
    pub config: Option<Config>,
    #[serde(skip)]
    pub draft_config: Option<Config>,
    #[serde(skip)]
    pub show_project_settings: bool,
    /// Project Flags pane visibility (Phase F).
    #[serde(skip)]
    pub show_project_flags: bool,
    /// Draft buffer for adding a friendly lemma in the Project Flags pane.
    #[serde(skip)]
    pub project_flags_friendly_draft: String,
    #[serde(skip)]
    pub llm_results_receiver: Option<Receiver<Result<Vec<(usize, String, String, String)>, String>>>,
    pub last_log: String,
    // LLM job progress
    #[serde(skip)]
    pub llm_job_total: usize,
    #[serde(skip)]
    pub llm_job_done: usize,
    #[serde(skip)]
    pub llm_cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    #[serde(skip)]
    pub llm_job_backup: Vec<(usize, String, String)>,
    #[serde(skip)]
    pub show_cancel_confirm: bool,
    // LLM UI state
    #[serde(skip)]
    pub show_llm_settings: bool,
    #[serde(skip)]
    pub show_llm_run: bool,
    #[serde(skip)]
    pub llm_run_start: usize,
    #[serde(skip)]
    pub llm_run_end: usize,
    #[serde(skip)]
    pub llm_run_batch_size: usize,
    #[serde(skip)]
    pub llm_run_prompt_name: String,
    // Output directory for generated files
    #[serde(skip)]
    pub output_dir: Option<String>,

    // Debug Dump dialog
    #[serde(skip)]
    pub show_debug_dump: bool,
    #[serde(skip)]
    pub debug_dump_start: usize,
    #[serde(skip)]
    pub debug_dump_end: usize,
    #[serde(skip)]
    pub debug_dump_path: String,

    // Config Set dialog
    #[serde(skip)]
    pub show_config_set: bool,
    #[serde(skip)]
    pub config_set_key: String,
    #[serde(skip)]
    pub config_set_value: String,

    // API Keys dialog
    #[serde(skip)]
    pub show_api_keys_dialog: bool,
    #[serde(skip)]
    pub api_key_anthropic_input: String,
    #[serde(skip)]
    pub api_key_google_input: String,

    // Command routing
    #[serde(skip)]
    pub pending_terminal_command: Option<String>,

    // New model alias input for the LLM Settings dialog
    #[serde(skip)]
    pub new_model_alias_input: String,

    // Metadata about the currently-running (or most-recently-run) LLM job.
    // Used when recording LlmCallRecords on success/failure.
    #[serde(skip)]
    pub llm_job_stage: String,
    #[serde(skip)]
    pub llm_job_target_tier: String,
    #[serde(skip)]
    pub llm_job_model: String,

    // Input buffer for the Proper Noun Lemmas editor tab
    #[serde(skip)]
    pub pn_lemma_input: String,

    // Per-segment edit buffers for the segment editor (key: "seg_id", value: current text)
    // Cleared whenever the selected sentence or tier view changes.
    #[serde(skip)]
    pub seg_edit_buffers: HashMap<String, String>,

    // Selected word indices (1-based) in the mapping table for merge/split/delete operations.
    #[serde(skip)]
    pub mapping_selected_rows: std::collections::BTreeSet<usize>,

    // Active tier for token-level commands (split/merge/insert/delete/edit_b).
    // Set via `select tier <tier>` command. Used by Bas B / Bas T editing.
    #[serde(skip)]
    pub selected_tier_id: String,

    // Follow-up command queue: when a basic tier is generated, mapping
    // generation commands are interleaved after each translation batch.
    // The GUI auto-advances through this queue as each sub-job completes.
    #[serde(skip)]
    pub llm_followup_queue: VecDeque<String>,

    // Co-pilot server info (set by the GUI when the relay server starts).
    // Format: (name, port). None if the server is not running.
    #[serde(skip)]
    pub copilot_server_info: Option<(String, u16)>,

    // Co-pilot LLM conversation history: Vec<(role, text)> where role is "user" or "assistant".
    #[serde(skip)]
    pub copilot_history: Vec<(String, String)>,

    // Co-pilot autonomous mode: when true, the copilot is executing a plan.
    #[serde(skip)]
    pub copilot_running: bool,

    // Co-pilot turn counter for the current session (reset on each new session).
    #[serde(skip)]
    pub copilot_turns: u32,

    // Co-pilot async LLM response channel (None when idle).
    #[serde(skip)]
    pub copilot_llm_rx: Option<Receiver<Result<String, String>>>,

    // Co-pilot auto-continue depth counter (reset each user message).
    #[serde(skip)]
    pub copilot_auto_turns: u32,

    // Co-pilot is waiting for a background AV job to finish before continuing.
    #[serde(skip)]
    pub copilot_awaiting_av: bool,

    // Co-pilot is waiting for a background LLM generation job to finish.
    #[serde(skip)]
    pub copilot_awaiting_llm_job: bool,

    // Remaining CMD: lines to execute after the current LLM job completes.
    #[serde(skip)]
    pub copilot_pending_cmds: Vec<String>,

    // Accumulated command outputs for the current copilot turn (before sending back to LLM).
    #[serde(skip)]
    pub copilot_cmd_outputs: Vec<String>,

    // Media (AV production) tab
    #[serde(skip)]
    pub show_media_tab: bool,

    // Currently selected stem in the Media tab (for chunk detail panel)
    #[serde(skip)]
    pub av_selected_stem: Option<String>,

    // Background AV generation job (audio or video subprocess)
    #[serde(skip)]
    pub av_job: Option<Arc<Mutex<AvJobState>>>,

    /// Whether the last audit passed without demotions.
    /// Any mutation (edit, validate, LLM result) resets this to false.
    /// generate_weave refuses to run unless this is true.
    #[serde(default)]
    pub audit_passed: bool,

    // --- Chapter Mode ---
    #[serde(default)]
    pub chapter_mode: bool,
    #[serde(default)]
    pub chapters: Vec<Chapter>,
    #[serde(skip)]
    pub selected_chapter_idx: Option<usize>,
}

fn default_languages() -> (String, String) {
    ("en".to_string(), "es".to_string())
}
fn default_left_view() -> TierView {
    TierView::AdvancedTarget
}
fn default_right_view() -> DetailView {
    DetailView::Tier(TierView::BasicBase)
}
fn default_sim_user_level() -> u32 {
    1
}
fn default_sim_manual_recipe() -> VLevelRecipe {
    VLevelRecipe { bas: 500, mod_v: 0, adv: 0 }
}
fn default_frontier_target_pct() -> f32 {
    5.0
}
fn default_frontier_seed() -> u64 {
    777
}
fn default_frontier_familiar_lemma_exclude_count() -> usize {
    100
}
fn default_true() -> bool {
    true
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            schema_version: 2, // New projects start at the post-wlemma schema.
            document: Vec::new(),
            selected_range: None,
            book_map: None,
            calibration_sentence_count: None,
            book_name: String::new(),
            frontier_enabled: true,
            frontier_target_pct: 5.0,
            frontier_seed: 777,
            frontier_test_mode: false,
            frontier_familiar_lemma_exclude_count: 100,
            project_languages: ("en".to_string(), "es".to_string()), // Default
            friendly_lemmas: Vec::new(),
            friendly_shielding_enabled: true,
            simple_mode: false,
            simple_triple: false,
            lesson_realign_enabled: false,
            source_is_basic: false,
            level_map_embedded: false,
            selected_sentence_idx: 0,
            left_view: TierView::AdvancedTarget,
            right_view: DetailView::Tier(TierView::BasicBase),
            sim_mode: SimulationMode::Manual,
            sim_user_level: 1,
            sim_manual_recipe: VLevelRecipe {
                bas: 500,
                mod_v: 0,
                adv: 0,
            },
            bridge: None,
            llm: None,
            prompts: None,
            logger: None,
            config: None,
            draft_config: None,
            tool_root_dir: None,
            llm_results_receiver: None,
            show_project_settings: false,
            show_project_flags: false,
            project_flags_friendly_draft: String::new(),
            show_llm_settings: false,
            show_llm_run: false,
            llm_run_start: 0,
            llm_run_end: 0,
            llm_run_batch_size: 10,
            llm_run_prompt_name: "GenerateBasicBase".to_string(),
            llm_job_total: 0,
            llm_job_done: 0,
            llm_cancel_flag: None,
            llm_job_backup: Vec::new(),
            show_cancel_confirm: false,
            last_log: "Ready.".to_string(),
            output_dir: None,
            show_debug_dump: false,
            debug_dump_start: 0,
            debug_dump_end: 0,
            debug_dump_path: String::new(),
            show_config_set: false,
            config_set_key: String::new(),
            config_set_value: String::new(),
            show_api_keys_dialog: false,
            api_key_anthropic_input: String::new(),
            api_key_google_input: String::new(),
            pending_terminal_command: None,
            new_model_alias_input: String::new(),
            llm_job_stage: String::new(),
            llm_job_target_tier: String::new(),
            llm_job_model: String::new(),
            pn_lemma_input: String::new(),
            seg_edit_buffers: HashMap::new(),
            mapping_selected_rows: std::collections::BTreeSet::new(),
            selected_tier_id: "basic_target".to_string(),
            llm_followup_queue: VecDeque::new(),
            copilot_server_info: None,
            copilot_history: Vec::new(),
            copilot_running: false,
            copilot_turns: 0,
            copilot_llm_rx: None,
            copilot_auto_turns: 0,
            copilot_awaiting_av: false,
            copilot_awaiting_llm_job: false,
            copilot_pending_cmds: Vec::new(),
            copilot_cmd_outputs: Vec::new(),
            show_media_tab: false,
            av_selected_stem: None,
            av_job: None,
            audit_passed: false,
            chapter_mode: false,
            chapters: Vec::new(),
            selected_chapter_idx: None,
        }
    }
}

// ... (Methods remain unchanged) ...
impl AppState {
    pub fn get_current_sentence(&self) -> Option<&Sentence> {
        self.document.get(self.selected_sentence_idx)
    }
    pub fn get_current_sentence_mut(&mut self) -> Option<&mut Sentence> {
        self.document.get_mut(self.selected_sentence_idx)
    }

    pub fn get_effective_recipe(&self) -> VLevelRecipe {
        match self.sim_mode {
            SimulationMode::Manual => self.sim_manual_recipe.clone(),
            SimulationMode::Calibrated => {
                if let Some(map) = &self.book_map {
                    let level_key = self.sim_user_level.to_string();
                    if let Some(curriculum) = map.get(&level_key) {
                        if let Some(entry) = curriculum.map.first() {
                            return entry.recipe.clone();
                        }
                    }
                }
                VLevelRecipe::default()
            }
        }
    }

    /// Derived: true when source language equals target language. Used to
    /// flip the basic-branch dependency direction (Spanish-source mode).
    pub fn source_is_target(&self) -> bool {
        !self.project_languages.0.is_empty()
            && self.project_languages.0 == self.project_languages.1
    }

    /// Stamp the current `source_is_target` value onto every sentence in
    /// the document so dependency-aware staleness propagation works.
    /// Call this after import, project load, or any change to
    /// `project_languages`.
    pub fn refresh_sentence_modes(&mut self) {
        let sit = self.source_is_target();
        let sm = self.simple_mode;
        for s in &mut self.document {
            s.set_source_is_target(sit);
            s.set_simple_mode(sm);
        }
    }

    /// Build a snapshot of the project-level flags for display in the
    /// terminal `flags` command and the GUI Project Flags pane.
    pub fn project_flags_summary(&self) -> ProjectFlagsSummary {
        ProjectFlagsSummary {
            source_language: self.project_languages.0.clone(),
            target_language: self.project_languages.1.clone(),
            source_is_target: self.source_is_target(),
            book_name: self.book_name.clone(),
            simple_mode: self.simple_mode,
            simple_triple: self.simple_triple,
            lesson_realign_enabled: self.lesson_realign_enabled,
            source_is_basic: self.source_is_basic,
            friendly_shielding_enabled: self.friendly_shielding_enabled,
            friendly_lemmas: self.friendly_lemmas.clone(),
            frontier_enabled: self.frontier_enabled,
            level_map_source: LevelMapSource::infer(self),
            teaching_mode_active: self.simple_mode
                && self.lesson_realign_enabled
                && !self.frontier_enabled
                && self.friendly_shielding_enabled,
        }
    }
}

// ---------------------------------------------------------------------------
// ProjectFlagsSummary — read-only snapshot for the `flags` command + GUI pane
// ---------------------------------------------------------------------------

/// Source attribution for the currently-loaded level map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelMapSource {
    /// No level map loaded.
    None,
    /// Embedded in the source text via `%%META lm_entry%%` directives.
    Embedded,
    /// Produced by `calibrate` or `import level_map` — `level_map_embedded` is false.
    Calibrated,
}

impl LevelMapSource {
    fn infer(state: &AppState) -> Self {
        if state.book_map.is_none() {
            Self::None
        } else if state.level_map_embedded {
            Self::Embedded
        } else {
            Self::Calibrated
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Embedded => "embedded",
            Self::Calibrated => "calibrated",
        }
    }
}

/// Snapshot of project-level flags. Cheap to construct; intended for
/// printing or rendering, not for mutation.
#[derive(Debug, Clone)]
pub struct ProjectFlagsSummary {
    pub source_language: String,
    pub target_language: String,
    pub source_is_target: bool,
    pub book_name: String,
    pub simple_mode: bool,
    pub simple_triple: bool,
    pub lesson_realign_enabled: bool,
    pub source_is_basic: bool,
    pub friendly_shielding_enabled: bool,
    pub friendly_lemmas: Vec<String>,
    pub frontier_enabled: bool,
    pub level_map_source: LevelMapSource,
    /// Derived: true when the underlying flags match the teaching_mode preset
    /// (simple_mode + lesson_realign + friendly_shielding + !frontier_enabled).
    pub teaching_mode_active: bool,
}

impl ProjectFlagsSummary {
    /// Render as a multi-line string for the terminal `flags` command.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("Project Flags\n");
        out.push_str("─────────────\n");
        out.push_str(&format!(
            "  Source language : {}\n",
            if self.source_language.is_empty() { "(unset)" } else { &self.source_language }
        ));
        out.push_str(&format!(
            "  Target language : {}\n",
            if self.target_language.is_empty() { "(unset)" } else { &self.target_language }
        ));
        out.push_str(&format!("  Source = Target : {}\n", self.source_is_target));
        out.push_str(&format!(
            "  Book name       : {}\n",
            if self.book_name.is_empty() { "(unset)" } else { &self.book_name }
        ));
        out.push_str(&format!(
            "  Teaching mode   : {}\n",
            if self.teaching_mode_active { "on" } else { "off (custom)" }
        ));
        out.push_str(&format!("  Simple mode     : {}\n", on_off(self.simple_mode)));
        out.push_str(&format!("  Simple-triple   : {}\n", on_off(self.simple_triple)));
        out.push_str(&format!("  Lesson realign  : {}\n", on_off(self.lesson_realign_enabled)));
        out.push_str(&format!("  Source is basic : {}\n", on_off(self.source_is_basic)));
        out.push_str(&format!(
            "  Friendly shield : {}\n",
            on_off(self.friendly_shielding_enabled)
        ));
        out.push_str(&format!(
            "  Friendly lemmas : {}\n",
            if self.friendly_lemmas.is_empty() {
                "(none)".to_string()
            } else {
                self.friendly_lemmas.join(", ")
            }
        ));
        out.push_str(&format!("  Frontier        : {}\n", on_off(self.frontier_enabled)));
        out.push_str(&format!("  Level map       : {}\n", self.level_map_source.as_str()));
        out
    }
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}
