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
use std::sync::mpsc::{Receiver};

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
    #[serde(default)]
    pub document: Vec<Sentence>,
    #[serde(default)]
    pub book_map: Option<HashMap<String, JsonCurriculumMap>>,
    #[serde(default)]
    pub book_name: String,

    // --- NEW FIELD: Project Languages (Base, Target) ---
    #[serde(default = "default_languages")]
    pub project_languages: (String, String),

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
    // When a job returns multiple results (collateral updates), we park them here for user review
    #[serde(skip)]
    pub pending_collateral_updates: Vec<(usize, String, String, String)>,
    #[serde(skip)]
    pub show_collateral_confirm: bool,

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

impl Default for AppState {
    fn default() -> Self {
        Self {
            document: Vec::new(),
            selected_range: None,
            book_map: None,
            book_name: String::new(),
            project_languages: ("en".to_string(), "es".to_string()), // Default
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
            llm_results_receiver: None,
            show_project_settings: false,
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
            pending_collateral_updates: Vec::new(),
            show_collateral_confirm: false,
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
}
