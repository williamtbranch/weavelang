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
use std::sync::mpsc::{Receiver};
use crate::services::llm_settings::StageBatchSettings;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimulationMode {
    Calibrated,
    Manual,
}

#[derive(Serialize, Deserialize)]
pub struct AppState {
    pub document: Vec<Sentence>,
    pub book_map: Option<HashMap<String, JsonCurriculumMap>>,
    pub book_name: String,

    // --- NEW FIELD: Project Languages (Base, Target) ---
    pub project_languages: (String, String),

    pub selected_sentence_idx: usize,
    // Optional contiguous selection range in the navigator (inclusive start,end).
    // If None, only `selected_sentence_idx` is selected.
    pub selected_range: Option<(usize, usize)>,
    pub left_view: TierView,
    pub right_view: DetailView,

    // Simulation Settings
    pub sim_mode: SimulationMode,
    pub sim_user_level: u32,
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
    pub llm_batch_settings: StageBatchSettings,
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

    // Command routing
    #[serde(skip)]
    pub pending_terminal_command: Option<String>,
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
            llm_results_receiver: None,
            llm_batch_settings: StageBatchSettings::default(),
            show_llm_settings: false,
            show_llm_run: false,
            llm_run_start: 0,
            llm_run_end: 0,
            llm_run_batch_size: StageBatchSettings::default().simplify,
            llm_run_prompt_name: "simplify_to_basic_english".to_string(),
            llm_job_total: 0,
            llm_job_done: 0,
            llm_cancel_flag: None,
            llm_job_backup: Vec::new(),
            show_cancel_confirm: false,
            last_log: "Ready.".to_string(),
            pending_collateral_updates: Vec::new(),
            show_collateral_confirm: false,
            pending_terminal_command: None,
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
