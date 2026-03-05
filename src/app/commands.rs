use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AppCommand {
    // Navigation & Selection
    SelectSentence { id: Option<String>, index: Option<usize> },
    SelectRange { start_id: Option<String>, end_id: Option<String>, start_index: Option<usize>, end_index: Option<usize> },
    SetRightView { view: String },
    SetLeftView { view: String },

    // Editing
    AddSentence,
    UpdateText { sentence_id: Option<String>, index: Option<usize>, tier_id: String, new_text: String },
    ApproveEdits { sentence_id: Option<String>, index: Option<usize>, tier_id: String },

    // Generation (LLM)
    GenerateTier { sentence_id: Option<String>, index: Option<usize>, tier_id: String },
    GenerateMapping { sentence_id: Option<String>, index: Option<usize>, source_tier: String, target_tier: String },
    GenerateStage { stage_name: String, start_index: usize, end_index: usize },
    ApplyCollateral { accept: bool },
    CheckStatus,
    ConfigSet { key: String, value: String },
    ConfigList,
    ConfigAddModel { alias: String },
    ConfigRemoveModel { alias: String },
    ConfigRenameModel { old_alias: String, new_alias: String },

    // Project Management
    OpenWorkspace { path: String },
    LoadProject { path: String },
    SaveProject { path: Option<String> },
    ImportSource { path: String },
    ImportJson { path: String },
    ExportJson { path: String },
    ExportLevelMap { path: String },
    
    // Measurement
    MeasureAvd { path: String },
    MeasureUserScore { path: String },

    // Level Map & Weave Generation
    ImportLevelMap { path: String },
    SetOutputDir { path: String },
    GenerateWeave { level: String },

    // API Key Management
    SetKey { provider: String, value: String },
    DeleteKey { provider: String },
    KeyStatus,

    // Debug / Testing
    DebugDump { start_index: usize, end_index: usize, path: Option<String> },

    // Terminal Specific (handled by CLI layer, but defined here for parsing convenience or separate enum)
    // Actually, terminal specific commands shouldn't be in AppCommand if they don't affect AppState.
    // We'll define a separate enum for TerminalCommand.
}

#[derive(Debug, Clone, PartialEq)]
pub enum TerminalCommand {
    ListNav { start_index: Option<usize> },
    ShowDetail,
    ShowMapping,
    WatchJob,
    Clear,
    History,
    Help,
    Exit,
    // Wrapper for AppCommand
    App(AppCommand),
}
