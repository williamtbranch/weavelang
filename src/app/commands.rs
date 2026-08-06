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
    GenerateStage { stage_name: String, start_index: usize, end_index: usize, no_followup: bool },
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
    /// The printable title of the work. Required before illustration prompts
    /// can be generated, because the thumbnail carries it.
    SetStoryTitle { title: String },
    OpenWorkspace { path: String },
    LoadProject { path: String },
    SaveProject { path: Option<String> },
    ImportSource { path: String },
    AppendSource { path: String, chapter_name: Option<String> },
    ImportJson { path: String },

    // Raw Source (ESCore-style adaptation)
    /// Import raw (usually English) text into the Raw Source tab.
    ImportRaw { path: String, chunk: Option<usize>, fresh: bool },
    /// Load the book-level approved-vocabulary policy file.
    AdaptDomain { path: String },
    /// Set an adaptation gate: coverage / ilevel / min / max / passes / gain.
    AdaptSet { key: String, value: String },
    /// Score the current draft(s) against the DRC without calling the LLM.
    AdaptScore { unit: Option<String> },
    /// One draft pass.
    AdaptDraft { unit: Option<String> },
    /// One squeeze pass.
    AdaptSqueeze { unit: Option<String> },
    /// Draft, then squeeze until PASS, floor, or the anti-churn ceiling.
    AdaptRun { unit: Option<String> },
    /// Show adaptation status for every unit.
    AdaptStatusReport,
    /// Show the full DRC report for one unit.
    AdaptReport { unit: Option<String> },
    /// Roll a unit's draft back one version.
    AdaptRevert { unit: Option<String> },
    /// Cancel the running adapt job.
    AdaptCancel,
    /// Force-write the resume checkpoint.
    AdaptCheckpoint,
    /// Reload a resume checkpoint after an interruption.
    AdaptRestore { path: Option<String> },
    /// Assemble the passing drafts into source text and import it.
    AdaptPromote { force: bool },

    ExportJson { path: String },
    ExportLevelMap { path: String },
    
    // Measurement
    MeasureAvd { path: String },
    MeasureUserScore { path: String },

    // Level Map & Weave Generation
    ImportLevelMap { path: String },
    SetOutputDir { path: String },
    GenerateWeave {
        level: String,
        force: bool,
        frontier_enabled_override: Option<bool>,
        frontier_target_pct_override: Option<f32>,
        frontier_seed_override: Option<u64>,
        frontier_test_mode_override: Option<bool>,
        frontier_familiar_lemma_exclude_count_override: Option<usize>,
        /// Study-format step size (only used when level == "sf")
        sf_step: Option<u32>,
        /// Study-format start level (only used when level == "sf")
        sf_start_level: Option<u32>,
    },
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
    /// Bulk accept mapping for sentences in a range where the tier is Stale.
    AcceptMapRange { start: usize, end: usize },
    /// Initialize an empty diglot mapping for the selected sentence + tier direction.
    InitMapping,

    /// Design Rule Check — validate all sentences for weave readiness.
    Drc,
    /// Design Rule Check filtered to a single tier. `limit` = None means 'all'.
    DrcTier { tier_id: String, limit: Option<usize> },

    /// Structural audit — demote Valid tiers that violate DRC rules.
    /// Can only invalidate, never promote. Respects editor intent.
    Audit,

    // Debug / Testing
    DebugDump { start_index: usize, end_index: usize, path: Option<String> },

    // Weave readiness & reporting (indices are 0-based internally)
    WeaveStatus,
    ReportSentencesIncomplete { limit: Option<usize> },
    ReportSentencesComplete { limit: Option<usize> },
    ReportSentence { start_index: usize, end_index: usize },

    // AV Production
    AvInit,
    AvStatus,
    AvMark { stems: Vec<String> },
    AvUnmark { stems: Vec<String> },
    AvMarkAll,
    AvClearMarks,
    AvGenerateAlign { target: AvTarget },
    AvGenerateAudio { target: AvTarget },
    AvGenerateVideo { target: AvTarget },
    AvGenerateCharacters { force: bool },
    AvGeneratePrompts { force: bool },
    AvGenerateIllustrations,
    AvConfigShow,
    AvConfigTts { key: String, value: String },
    AvConfigVideo { key: String, value: String },
    AvConfigIllustrations { key: String, value: String },
    AvConfigVoices { voices: Vec<String> },
    AvOpenDir { which: String },
    AvCancel,
    AvLog { tail: Option<usize> },
    AvRejectChunk { stem: String, index: u32 },
    AvRestoreChunk { stem: String, index: u32 },
    AvChunkStatus { stem: String },
    AvRebuildAudio { stem: String },

    // YouTube
    AvYoutubeInit,
    AvYoutubeAuth,
    AvYoutubeConfigShow,
    AvYoutubeConfig { key: String, value: String },
    AvYoutubeUpload { target: AvTarget },

    // Study Format
    AvSfPreflight { stem: Option<String> },
    AvSfBuild { target: AvTarget },

    // Level Map Inspection
    ShowLevelMap { level: Option<u32> },

    // Frontier Settings
    SetFrontierEnabled { enabled: bool },
    SetFrontierPct { pct: f32 },
    SetFrontierSeed { seed: u64 },

    // Chapter Mode
    NewChapter { name: String, start: usize, end: usize },
    ListChapters,
    DeleteChapter { name: String },
    SelectChapter { name: String },
    SetChapterMode { enabled: bool },
    InitMediaWorkspace,
    /// Append a timestamped entry to copilot/_journal.md.
    CopilotJournal { text: String },
    /// Clear copilot session history (start fresh).
    CopilotReset,

    // Project flags (Phase F)
    /// Print the read-only project flags pane.
    ShowFlags,
    /// Add a friendly lemma (case-insensitive de-dup).
    SetFriendlyLemma { lemma: String },
    /// Remove a friendly lemma (case-insensitive match).
    UnsetFriendlyLemma { lemma: String },
    /// Clear the friendly-lemmas list.
    ClearFriendlyLemmas,
    /// Toggle simple_mode.
    SetSimpleMode { enabled: bool },
    /// Toggle simple_triple output mode (basic_base off; only basic_target woven).
    SetSimpleTriple { enabled: bool },
    /// Toggle single_simple output mode (basic_base off; only basic_target
    /// woven; advanced/moderate never produced; default `dg` V24 output).
    SetSingleSimple { enabled: bool },
    /// Toggle source_is_basic.
    SetSourceIsBasic { enabled: bool },
    /// Toggle lesson_realign post-processing for chapter lesson audio.
    SetLessonRealign { enabled: bool },
    /// Toggle friendly_shielding_enabled.
    SetFriendlyShielding { enabled: bool },
    /// Teaching-mode preset: on = simple_mode=on + frontier_enabled=off (and
    /// asserts friendly_shielding_enabled=on); off is a no-op (does not
    /// unset the underlying flags).
    SetTeachingMode { enabled: bool },
    /// Clear the loaded level map and the `level_map_embedded` flag.
    /// Escape hatch for re-calibrating an imported lesson.
    StripLevelMap,
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
    /// Show context around a specific sentence: `list nav --around <N>`
    ListNavAround { center: usize },
    /// Search for text in base tier: `search "<text>"`
    SearchText { query: String },
    /// Scan for chapter-like headings in the document.
    ListHeadings,
    /// Show calibration metadata (sentence count, whether recalibration needed).
    CalibrationInfo,
    ShowDetail,
    ShowMapping,
    ShowTokens,
    /// Print the current view (selected tier) or a specific tier without switching.
    Print { tier: Option<String> },
    WatchJob,
    JobStatus,
    Clear,
    History,
    Help,
    AvHelp,
    Exit,
    /// Show copilot server name and port.
    ServerInfo,
    /// T7.1: Inspect the wlemma bucket for a given lemma. Shows the
    /// bucket key (stem), its rank, and every member of the bucket.
    WlemmaInspect { lemma: String },
    // Wrapper for AppCommand
    App(AppCommand),
}
