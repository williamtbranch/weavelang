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
    AddSentenceWithText { text: String },
    RemoveSentence { index: usize },
    UpdateText { sentence_id: Option<String>, index: Option<usize>, tier_id: String, new_text: String },
    ApproveEdits { sentence_id: Option<String>, index: Option<usize>, tier_id: String },
    /// Approve a tier as Valid — lemmatizes if Python bridge is available.
    ApproveTier { index: usize, tier_id: String },

    // Generation (LLM)
    GenerateTier { sentence_id: Option<String>, index: Option<usize>, tier_id: String },
    GenerateMapping { sentence_id: Option<String>, index: Option<usize>, source_tier: String, target_tier: String },
    GenerateStage { stage_name: String, start_index: usize, end_index: usize },
    CheckStatus,
    ConfigSet { key: String, value: String },
    ConfigList,
    ConfigAddModel { alias: String },
    ConfigRemoveModel { alias: String },
    ConfigRenameModel { old_alias: String, new_alias: String },

    // Project Management
    NewProject { name: String },
    CloseProject,
    SetLanguages { source: String, target: String },
    SetBookName { name: String },
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
    GenerateWeave { level: String, force: bool },
    Calibrate { max_level: Option<u32> },

    // API Key Management
    SetKey { provider: String, value: String },
    DeleteKey { provider: String },
    KeyStatus,

    // Proper Noun Lemma Editing
    ListPnLemmas { index: usize },
    AddPnLemma { index: usize, lemma: String },
    RemovePnLemma { index: usize, lemma: String },

    // Segment-level Editing (for Adv/Mod tiers)
    EditSegment { index: usize, tier_id: String, seg_id: String, new_text: String },
    AddSegment { index: usize, tier_id: String, after_seg_id: String, new_text: String },
    RemoveSegment { index: usize, tier_id: String, seg_id: String },
    LemmatizeTier { index: usize, tier_id: String },
    ValidateTier { index: usize, tier_id: String },

    // Token-level mapping editing (for Bas B / Bas T tiers)
    // These operate on the currently selected sentence + selected tier.
    /// Selects the active tier for subsequent token commands.
    SelectTier { tier_id: String },
    /// Split word token at 1-based word index into sub-tokens via re-tokenize.
    SplitToken { word_index: usize },
    /// Merge word tokens from word_start..=word_end (1-based) into one.
    MergeTokens { word_start: usize, word_end: usize },
    /// Insert an empty word token at 1-based word position (shifts others down).
    InsertToken { word_index: usize },
    /// Delete word token at 1-based word index, merging surrounding backgrounds.
    DeleteToken { word_index: usize },
    /// Edit the source word text at word_index (1-based).
    EditWord { word_index: usize, new_text: String },
    /// Edit the background token immediately before word at word_index (1-based).
    /// word_index can be N+1 (one past last word) to address the trailing background.
    EditBackground { word_index: usize, new_text: String },
    /// Edit the target text of a mapping entry for the word at word_index (1-based).
    EditTarget { word_index: usize, new_text: String },
    /// Batch edit multiple mapping targets: pairs of (1-based word_index, text).
    EditTargets { pairs: Vec<(usize, String)> },
    /// Edit the full text of the currently selected sentence + tier.
    /// Resolves sentence from selected_sentence_idx, tier from selected_tier_id.
    EditText { new_text: String },
    /// Accept / validate the mapping for the selected tier, marking it complete.
    AcceptMap,
    /// Initialize an empty diglot mapping for the selected sentence + tier direction.
    InitMapping,

    /// Design Rule Check — validate all sentences for weave readiness.
    Drc,

    /// Structural audit — demote Valid tiers that violate DRC rules.
    /// Can only invalidate, never promote. Respects editor intent.
    Audit,

    // Debug / Testing
    DebugDump { start_index: usize, end_index: usize, path: Option<String> },

    // Weave readiness & reporting (indices are 0-based internally)
    WeaveStatus,
    ReportSentencesIncomplete,
    ReportSentencesComplete,
    ReportSentence { start_index: usize, end_index: usize },

    // AV Production
    AvInit,
    AvStatus,
    AvMark { stems: Vec<String> },
    AvUnmark { stems: Vec<String> },
    AvMarkAll,
    AvClearMarks,
    AvGenerateAudio { target: AvTarget },
    AvGenerateVideo { target: AvTarget },
    AvConfigShow,
    AvConfigTts { key: String, value: String },
    AvConfigVideo { key: String, value: String },
    AvConfigVoices { voices: Vec<String> },
    AvOpenDir { which: String },
    AvCancel,
    AvRejectChunk { stem: String, index: u32 },
    AvRestoreChunk { stem: String, index: u32 },
    AvChunkStatus { stem: String },
    AvRebuildAudio { stem: String },

    // Level Map Inspection
    ShowLevelMap { level: Option<u32> },

    // Chapter Mode
    NewChapter { name: String, start: usize, end: usize },
    ListChapters,
    DeleteChapter { name: String },
    SelectChapter { name: String },
    SetChapterMode { enabled: bool },
    InitMediaWorkspace,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AvTarget {
    Stem(String),
    Next,
    All,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TerminalCommand {
    ListNav { start_index: Option<usize> },
    ShowDetail,
    ShowMapping,
    ShowTokens,
    /// Print the current view (selected tier) or a specific tier without switching.
    Print { tier: Option<String> },
    WatchJob,
    Clear,
    History,
    Help,
    AvHelp,
    Exit,
    /// Show copilot server name and port.
    ServerInfo,
    // Wrapper for AppCommand
    App(AppCommand),
}
