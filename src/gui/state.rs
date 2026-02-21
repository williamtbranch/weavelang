// src/gui/state.rs

use crate::domain::sentence::Sentence;
use crate::simulation::numerical_types::VLevelRecipe;
use crate::types::json_types::JsonCurriculumMap;
use crate::services::python_bridge::BridgeService;
use crate::services::llm_client::LlmService;
use crate::services::prompt_manager::PromptManager;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::services::llm_logger::LlmLogger;

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

    // --- NEW FIELD: Project Languages (Base, Target) ---
    pub project_languages: (String, String),

    pub selected_sentence_idx: usize,
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
    pub last_log: String, 
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            document: Vec::new(),
            book_map: None,
            project_languages: ("en".to_string(), "es".to_string()), // Default
            selected_sentence_idx: 0,
            left_view: TierView::AdvancedTarget,
            right_view: DetailView::Tier(TierView::BasicBase),
            sim_mode: SimulationMode::Manual,
            sim_user_level: 1,
            sim_manual_recipe: VLevelRecipe { bas: 500, mod_v: 0, adv: 0 },
            bridge: None,
            llm: None,
            prompts: None,
            logger: None,
            last_log: "Ready.".to_string(),
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