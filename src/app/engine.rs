use crate::app::commands::{AppCommand, AvTarget};
use crate::app::state::AppState;
use crate::domain::tier::TierState;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use crate::parsing::source_parser;
use crate::types::json_types::JsonChapter;

/// Run tier-level DRC for a specific tier on a sentence.
/// Returns a Vec of violation strings. Empty means pass.
fn drc_tier(sent: &crate::domain::sentence::Sentence, tier_id: &str, sn: usize) -> Vec<String> {
    let mut violations = Vec::new();
    let sid = &sent.id;

    if tier_id == "basic_base" {
        let fwd = sent.mappings.iter()
            .find(|m| m.from_tier_id == "basic_base" && m.to_tier_id == "basic_target");
        match fwd {
            None => violations.push(format!("S{} ({}): forward mapping (basic_base→basic_target) is missing", sn, sid)),
            Some(mapping) if mapping.entries.is_empty() => {
                violations.push(format!("S{} ({}): forward mapping has 0 entries", sn, sid));
            }
            Some(mapping) => {
                if let Some(tier) = sent.tiers.get("basic_base") {
                    if let Some(seg) = tier.segments.first() {
                        let wc = seg.stream.words_enumerated().len();
                        if mapping.entries.len() < wc {
                            violations.push(format!("S{} ({}): forward mapping covers {}/{} words", sn, sid, mapping.entries.len(), wc));
                        }
                    }
                }
            }
        }
    }
    if tier_id == "basic_target" {
        let inv = sent.mappings.iter()
            .find(|m| m.from_tier_id == "basic_target" && m.to_tier_id == "basic_base");
        match inv {
            None => violations.push(format!("S{} ({}): inverse mapping (basic_target→basic_base) is missing", sn, sid)),
            Some(mapping) if mapping.entries.is_empty() => {
                violations.push(format!("S{} ({}): inverse mapping has 0 entries", sn, sid));
            }
            Some(mapping) => {
                if let Some(tier) = sent.tiers.get("basic_target") {
                    if let Some(seg) = tier.segments.first() {
                        let wc = seg.stream.words_enumerated().len();
                        if mapping.entries.len() < wc {
                            violations.push(format!("S{} ({}): inverse mapping covers {}/{} words", sn, sid, mapping.entries.len(), wc));
                        }
                    }
                }
            }
        }
    }

    violations
}

/// Run sentence-level DRC rules that apply only when all tiers are Valid.
/// Returns a Vec of violation strings. Empty means pass.
fn drc_sentence(sent: &crate::domain::sentence::Sentence, sn: usize) -> Vec<String> {
    let mut violations = Vec::new();
    let sid = &sent.id;

    let adv_seg_count = sent.tiers.get("advanced_target").map(|t| t.segments.len());
    let mod_seg_count = sent.tiers.get("moderate_target").map(|t| t.segments.len());
    if let (Some(a), Some(m)) = (adv_seg_count, mod_seg_count) {
        if a != m {
            violations.push(format!(
                "S{} ({}): advanced_target has {} segments but moderate_target has {} (must match)",
                sn, sid, a, m
            ));
        }
    }

    violations
}

/// Lemmatize all segments within a tier using SpaCy, updating word lemmas
/// and collecting segment/tier-level lemma lists.
/// Skips base-language tiers (base, basic_base) — no need for English lemmas.
pub(crate) fn lemmatize_tier_segments(
    sent: &mut crate::domain::sentence::Sentence,
    tier_id: &str,
    bridge: &crate::services::python_bridge::BridgeService,
    tier_lang: &str,
) -> Result<(), String> {
    if tier_id == "base" || tier_id == "basic_base" {
        return Ok(());
    }
    let tier = sent.tiers.get_mut(tier_id)
        .ok_or_else(|| format!("Tier '{}' not found.", tier_id))?;
    for seg in tier.segments.iter_mut() {
        let text = seg.full_text();
        let raw_tokens = bridge.tokenize(&text, tier_lang)
            .map_err(|e| format!("Tokenize failed for segment {}: {}", seg.id, e))?;
        seg.stream.update_lemmas_from_spacy(raw_tokens)
            .map_err(|e| format!("Lemma update failed for segment {}: {}", seg.id, e))?;
        seg.lemmas = seg.stream.tokens().iter()
            .filter_map(|t| match t {
                crate::domain::token_stream::Token::Word(wd) => Some(wd.lemmas.clone()),
                _ => None,
            })
            .flatten()
            .collect();
    }
    tier.lemmas = tier.segments.iter()
        .flat_map(|s| s.lemmas.clone())
        .collect();
    tier.lemmas.sort();
    tier.lemmas.dedup();
    Ok(())
}

/// Populate target_lemmas on mapping entries for L1 tiers (basic_base, basic_target).
/// Lemmas are ALWAYS target-language (Spanish for en-es) because the core algorithm
/// checks them against the student's known vocabulary.
///
/// - basic_base → basic_target: target_text IS Spanish, so tokenize it with target_lang.
/// - basic_target → basic_base: target_text is English (no use for English lemmas).
///   Instead, copy the Spanish lemmas from the SOURCE word in the basic_target stream.
pub(crate) fn lemmatize_mapping_targets(
    sent: &mut crate::domain::sentence::Sentence,
    tier_id: &str,
    bridge: &crate::services::python_bridge::BridgeService,
    _source_lang: &str,
    target_lang: &str,
) -> Result<(), String> {
    if tier_id == "basic_base" {
        // Forward mapping: target_text is Spanish → tokenize to get Spanish lemmas
        if let Some(mapping) = sent.mappings.iter_mut()
            .find(|m| m.from_tier_id == "basic_base" && m.to_tier_id == "basic_target")
        {
            for entry in mapping.entries.iter_mut() {
                if entry.target_text.trim().is_empty() {
                    continue;
                }
                let raw_tokens = bridge.tokenize(&entry.target_text, target_lang)
                    .map_err(|e| format!("Mapping lemmatize failed for '{}': {}", entry.target_text, e))?;
                let lemmas: Vec<String> = raw_tokens.iter()
                    .filter(|t| !t.is_punct && !t.is_space)
                    .map(|t| if t.lemma.is_empty() { t.text.clone() } else { t.lemma.clone() })
                    .collect();
                entry.target_lemmas = lemmas;
            }
        }
    } else if tier_id == "basic_target" {
        // Inverse mapping: target_text is English — we don't want English lemmas.
        // Copy Spanish lemmas from the source word in the basic_target token stream.
        let word_lemma_map: std::collections::HashMap<crate::domain::primitives::WordId, Vec<String>> =
            if let Some(tier) = sent.tiers.get("basic_target") {
                tier.segments.iter()
                    .flat_map(|seg| seg.stream.tokens().iter().filter_map(|t| {
                        if let crate::domain::token_stream::Token::Word(wd) = t {
                            Some((wd.id, wd.lemmas.clone()))
                        } else {
                            None
                        }
                    }).collect::<Vec<_>>())
                    .collect()
            } else {
                std::collections::HashMap::new()
            };

        if let Some(mapping) = sent.mappings.iter_mut()
            .find(|m| m.from_tier_id == "basic_target" && m.to_tier_id == "basic_base")
        {
            for entry in mapping.entries.iter_mut() {
                if let Some(lemmas) = word_lemma_map.get(&entry.source_word_id) {
                    entry.target_lemmas = lemmas.clone();
                }
            }
        }
    }

    Ok(())
}

pub struct Engine {
    pub state: AppState,
    pub current_file_path: Option<PathBuf>,
}

impl Engine {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            current_file_path: None,
        }
    }

    /// Returns the tool root directory (where `assets/` lives).
    /// Prefers the stored value; falls back to `std::env::current_dir()`.
    fn tool_root(&self) -> Result<PathBuf, String> {
        if let Some(ref root) = self.state.tool_root_dir {
            Ok(root.clone())
        } else {
            std::env::current_dir()
                .map_err(|e| format!("Cannot determine project root (no tool_root_dir set): {e}"))
        }
    }

    /// Resolve a user-supplied path against the workspace directory.
    /// Absolute paths are returned as-is; relative paths are resolved
    /// relative to `content_project_dir` (the open workspace) and
    /// canonicalized to prevent `..` traversal outside the workspace.
    fn resolve_path(&self, path: &str) -> PathBuf {
        let p = PathBuf::from(path);
        if p.is_absolute() {
            p
        } else if let Some(cfg) = &self.state.config {
            let base = cfg.content_project_dir_path();
            let joined = base.join(path);
            // Canonicalize to resolve any ".." components.  If the path
            // doesn't exist yet (common for new exports), clean it manually.
            let resolved = joined.canonicalize().unwrap_or_else(|_| {
                // Strip ".." components from the logical path
                let mut clean = PathBuf::new();
                for component in joined.components() {
                    match component {
                        std::path::Component::ParentDir => { clean.pop(); }
                        c => clean.push(c.as_os_str()),
                    }
                }
                clean
            });
            // Ensure the resolved path is still within the workspace.
            if !resolved.starts_with(&base) {
                eprintln!(
                    "[SECURITY] Path '{}' resolves outside workspace '{}'. Clamping to workspace root.",
                    path, base.display()
                );
                base.to_path_buf()
            } else {
                resolved
            }
        } else {
            p
        }
    }

    pub fn execute(&mut self, command: AppCommand) -> Result<String, String> {
        // Only commands that mutate sentence/tier/mapping content should
        // invalidate audit. Non-content operations (AV, weave export,
        // calibration/import/export/config/navigation) must keep audit state.
        let invalidates_audit = matches!(
            &command,
            AppCommand::AddSentence
            | AppCommand::AddSentenceWithText { .. }
            | AppCommand::RemoveSentence { .. }
            | AppCommand::UpdateText { .. }
            | AppCommand::ApproveEdits { .. }
            | AppCommand::ApproveTier { .. }
            | AppCommand::GenerateTier { .. }
            | AppCommand::GenerateMapping { .. }
            | AppCommand::GenerateStage { .. }
            | AppCommand::SetLanguages { .. }
            | AppCommand::SetBookName { .. }
            | AppCommand::ImportSource { .. }
            | AppCommand::ImportJson { .. }
            | AppCommand::AddPnLemma { .. }
            | AppCommand::RemovePnLemma { .. }
            | AppCommand::EditSegment { .. }
            | AppCommand::AddSegment { .. }
            | AppCommand::RemoveSegment { .. }
            | AppCommand::LemmatizeTier { .. }
            | AppCommand::ValidateTier { .. }
            | AppCommand::SplitToken { .. }
            | AppCommand::MergeTokens { .. }
            | AppCommand::InsertToken { .. }
            | AppCommand::DeleteToken { .. }
            | AppCommand::EditWord { .. }
            | AppCommand::EditBackground { .. }
            | AppCommand::EditTarget { .. }
            | AppCommand::EditTargets { .. }
            | AppCommand::EditText { .. }
            | AppCommand::AcceptMap
            | AppCommand::AcceptMapRange { .. }
            | AppCommand::InitMapping
        );
        if invalidates_audit {
            self.state.audit_passed = false;
        }

        match command {
            AppCommand::SelectSentence { id, index } => {
                if let Some(idx) = index {
                    if idx < self.state.document.len() {
                        self.state.selected_sentence_idx = idx;
                        self.state.selected_range = None;
                        return Ok(format!("Selected sentence {}", idx + 1));
                    }
                    return Err(format!("Sentence {} out of range (1-{})", idx + 1, self.state.document.len()));
                } else if let Some(sid) = id {
                    if let Some(idx) = self.state.document.iter().position(|s| s.id == sid) {
                        self.state.selected_sentence_idx = idx;
                        self.state.selected_range = None;
                        return Ok(format!("Selected sentence id {}", sid));
                    }
                    return Err("Sentence ID not found".to_string());
                }
                Err("Must provide id or index".to_string())
            }
            AppCommand::SelectRange { start_id: _start_id, end_id: _end_id, start_index: _start_index, end_index: _end_index } => {
                // TODO: Implement range selection
                Ok("Range selected".to_string())
            }
            AppCommand::SetRightView { view } => {
                use crate::app::state::{DetailView, TierView};
                match view.as_str() {
                    "base" => self.state.right_view = DetailView::Tier(TierView::Base),
                    "advanced_target" => self.state.right_view = DetailView::Tier(TierView::AdvancedTarget),
                    "moderate_target" => self.state.right_view = DetailView::Tier(TierView::ModerateTarget),
                    "basic_target" => self.state.right_view = DetailView::Tier(TierView::BasicTarget),
                    "basic_base" => self.state.right_view = DetailView::Tier(TierView::BasicBase),
                    "simulation" => self.state.right_view = DetailView::Tier(TierView::Simulation),
                    "token_base" => self.state.right_view = DetailView::Token(TierView::Base),
                    "token_advanced_target" => self.state.right_view = DetailView::Token(TierView::AdvancedTarget),
                    "token_moderate_target" => self.state.right_view = DetailView::Token(TierView::ModerateTarget),
                    "token_basic_target" => self.state.right_view = DetailView::Token(TierView::BasicTarget),
                    "token_basic_base" => self.state.right_view = DetailView::Token(TierView::BasicBase),
                    "token_simulation" => self.state.right_view = DetailView::Token(TierView::Simulation),
                    "mapping_diglot" => self.state.right_view = DetailView::MappingDiglot,
                    "mapping_inverse" => self.state.right_view = DetailView::MappingInverse,
                    "proper_noun_lemmas" => self.state.right_view = DetailView::ProperNounLemmas,
                    _ => return Err(format!("Unknown view: {}", view)),
                }
                // Keep selected_tier_id in sync when switching to a tier-based view
                match self.state.right_view {
                    DetailView::Tier(tv) | DetailView::Token(tv) => {
                        let tid = match tv {
                            TierView::Base => "base",
                            TierView::AdvancedTarget => "advanced_target",
                            TierView::ModerateTarget => "moderate_target",
                            TierView::BasicTarget => "basic_target",
                            TierView::BasicBase => "basic_base",
                            TierView::Simulation => "simulation",
                        };
                        self.state.selected_tier_id = tid.to_string();
                    }
                    _ => {} // mapping/pn views don't map to a single tier
                }
                Ok(format!("Right view set to {}", view))
            }
            AppCommand::SetLeftView { view } => {
                use crate::app::state::TierView;
                match view.as_str() {
                    "base" => self.state.left_view = TierView::Base,
                    "advanced_target" => self.state.left_view = TierView::AdvancedTarget,
                    "moderate_target" => self.state.left_view = TierView::ModerateTarget,
                    "basic_target" => self.state.left_view = TierView::BasicTarget,
                    "basic_base" => self.state.left_view = TierView::BasicBase,
                    "simulation" => self.state.left_view = TierView::Simulation,
                    _ => return Err(format!("Unknown view: {}", view)),
                }
                Ok(format!("Left view set to {}", view))
            }
            AppCommand::AddSentence => {
                use crate::domain::sentence::Sentence;
                use crate::domain::tier::Tier;
                use crate::domain::segment::Segment;
                use crate::domain::token_stream::TokenStream;

                let new_id = format!("S{}", self.state.document.len() + 1);
                let mut sentence = Sentence::new(new_id.clone());

                let mut tier = Tier::new("base".to_string());
                tier.add_segment(Segment::from_stream(
                    "S1".to_string(),
                    TokenStream::new(""),
                    vec![],
                ));
                sentence.add_tier(tier);

                self.state.document.push(sentence);
                self.state.selected_sentence_idx = self.state.document.len() - 1;
                self.state.selected_range = None;
                Ok(format!("Added new sentence {}", new_id))
            }
            AppCommand::AddSentenceWithText { text } => {
                use crate::domain::sentence::Sentence;
                use crate::domain::tier::Tier;
                use crate::domain::segment::Segment;
                use crate::domain::token_stream::TokenStream;

                let new_id = format!("S{}", self.state.document.len() + 1);
                let mut sentence = Sentence::new(new_id.clone());

                let mut tier = Tier::new("base".to_string());
                tier.add_segment(Segment::from_stream(
                    "S1".to_string(),
                    TokenStream::new(&text),
                    vec![],
                ));
                sentence.add_tier(tier);

                self.state.document.push(sentence);
                self.state.selected_sentence_idx = self.state.document.len() - 1;
                self.state.selected_range = None;
                Ok(format!("Added sentence {} with base text: {}", new_id, text))
            }
            AppCommand::RemoveSentence { index } => {
                if index >= self.state.document.len() {
                    return Err(format!("Sentence index {} out of range (have {})", index + 1, self.state.document.len()));
                }
                let removed_id = self.state.document[index].id.clone();
                self.state.document.remove(index);
                // Fix selection
                if self.state.document.is_empty() {
                    self.state.selected_sentence_idx = 0;
                } else if self.state.selected_sentence_idx >= self.state.document.len() {
                    self.state.selected_sentence_idx = self.state.document.len() - 1;
                }
                self.state.selected_range = None;
                Ok(format!("Removed sentence {} (was index {})", removed_id, index + 1))
            }
            AppCommand::NewProject { name } => {
                self.state.document.clear();
                self.state.book_name = name.clone();
                self.state.book_map = None;
                self.state.selected_sentence_idx = 0;
                self.state.selected_range = None;
                self.state.last_log = format!("New project: {}", name);
                self.state.output_dir = None;
                self.current_file_path = None;
                self.state.llm_followup_queue.clear();
                self.state.chapters.clear();
                self.state.chapter_mode = false;
                self.state.selected_chapter_idx = None;
                Ok(format!("Created new project '{}'", name))
            }
            AppCommand::CloseProject => {
                let old_name = if self.state.book_name.is_empty() {
                    "(unnamed)".to_string()
                } else {
                    self.state.book_name.clone()
                };
                self.state.document.clear();
                self.state.book_name = String::new();
                self.state.book_map = None;
                self.state.selected_sentence_idx = 0;
                self.state.selected_range = None;
                self.state.output_dir = None;
                self.current_file_path = None;
                self.state.llm_followup_queue.clear();
                self.state.chapters.clear();
                self.state.chapter_mode = false;
                self.state.selected_chapter_idx = None;
                self.state.last_log = "Project closed.".to_string();
                Ok(format!("Closed project '{}'", old_name))
            }
            AppCommand::SetLanguages { source, target } => {
                self.state.project_languages = (source.clone(), target.clone());
                Ok(format!("Languages set to {} → {}", source, target))
            }
            AppCommand::SetBookName { name } => {
                self.state.book_name = name.clone();
                Ok(format!("Book name set to '{}'", name))
            }
            AppCommand::UpdateText { sentence_id, index, tier_id, new_text } => {
                let idx = if let Some(i) = index {
                    i
                } else if let Some(sid) = sentence_id {
                    self.state.document.iter().position(|s| s.id == sid).ok_or("Sentence ID not found")?
                } else {
                    return Err("Must provide id or index".to_string());
                };

                if let Some(sent) = self.state.document.get_mut(idx) {
                    sent.update_tier_text(&tier_id, new_text);
                    Ok(format!("Updated text for sentence index {}, tier {}", idx, tier_id))
                } else {
                    Err("Index out of bounds".to_string())
                }
            }
            AppCommand::ApproveEdits { sentence_id, index, tier_id } => {
                let idx = if let Some(i) = index {
                    i
                } else if let Some(sid) = sentence_id {
                    self.state.document.iter().position(|s| s.id == sid).ok_or("Sentence ID not found")?
                } else {
                    return Err("Must provide id or index".to_string());
                };

                // Delegate to ApproveTier so lemmatization always runs
                self.execute(AppCommand::ApproveTier { index: idx, tier_id })
            }
            AppCommand::ApproveTier { index, tier_id } => {
                // Resolve sentinel: usize::MAX + empty tier_id means "use current selection"
                let index = if index == usize::MAX { self.state.selected_sentence_idx } else { index };
                let tier_id = if tier_id.is_empty() { self.state.selected_tier_id.clone() } else { tier_id };
                if tier_id.is_empty() {
                    return Err("No tier selected. Use 'select tier <name>' first, or 'approve tier <N> <tier>'.".to_string());
                }
                // Lemmatization is MANDATORY for the Valid state.
                // Uses non-destructive in-place lemma update to preserve
                // phrasal segments (Adv/Mod) and mapping-aligned tokens (Bas B/T).
                let bridge = self.state.bridge.as_ref()
                    .ok_or_else(|| "Python bridge not available. Cannot approve tier without lemmatization.".to_string())?;
                let source_lang = self.state.project_languages.0.clone();
                let target_lang = self.state.project_languages.1.clone();
                // Tier text language: base/basic_base are source, all others are target
                let tier_lang = if tier_id == "base" || tier_id == "basic_base" {
                    &source_lang
                } else {
                    &target_lang
                };

                let sent = self.state.document.get_mut(index)
                    .ok_or_else(|| "Index out of bounds".to_string())?;

                // Phase 1: Lemmatize tier segments (skip for base-language tiers — no need for English lemmas)
                lemmatize_tier_segments(sent, &tier_id, bridge, tier_lang)?;

                // Phase 1b: Lemmatize mapping target texts for L1 tiers
                lemmatize_mapping_targets(sent, &tier_id, bridge, &source_lang, &target_lang)?;

                // Phase 2: DRC checks (immutable access to sent)
                let tier_violations = drc_tier(sent, &tier_id, index + 1);
                if !tier_violations.is_empty() {
                    return Err(format!("DRC failed for tier '{}' on sentence {}:\n  {}", tier_id, index + 1, tier_violations.join("\n  ")));
                }
                let other_tiers_valid = crate::domain::sentence::Sentence::WEAVE_TIERS.iter()
                    .filter(|&&t| t != tier_id)
                    .all(|&t| sent.tiers.get(t).map_or(false, |tier| tier.state == crate::domain::tier::TierState::Valid));
                if other_tiers_valid {
                    let sent_violations = drc_sentence(sent, index + 1);
                    if !sent_violations.is_empty() {
                        return Err(format!("Sentence-level DRC failed for sentence {}:\n  {}", index + 1, sent_violations.join("\n  ")));
                    }
                }

                // Phase 3: Mark Valid
                let tier = sent.tiers.get_mut(&tier_id).unwrap();
                tier.state = crate::domain::tier::TierState::Valid;
                sent.propagate_stale_from(&tier_id);

                Ok(format!("Tier '{}' on sentence {} lemmatized and approved as Valid.", tier_id, index + 1))
            }
            AppCommand::GenerateTier { sentence_id: _sentence_id, index: _index, tier_id: _tier_id } => {
                // TODO: Implement tier generation
                Ok("Tier generation started".to_string())
            }
            AppCommand::GenerateMapping { sentence_id: _sentence_id, index: _index, source_tier: _source_tier, target_tier: _target_tier } => {
                // TODO: Implement mapping generation
                Ok("Mapping generation started".to_string())
            }
            AppCommand::ListPnLemmas { index } => {
                if let Some(sent) = self.state.document.get(index) {
                    let lemmas = &sent.proper_noun_lemmas;
                    if lemmas.is_empty() {
                        Ok(format!("Sentence {} has no proper noun lemmas.", index + 1))
                    } else {
                        let list = lemmas.join(", ");
                        Ok(format!("Sentence {} PN lemmas: {}", index + 1, list))
                    }
                } else {
                    Err("Index out of bounds".to_string())
                }
            }
            AppCommand::AddPnLemma { index, lemma } => {
                if let Some(sent) = self.state.document.get_mut(index) {
                    if sent.proper_noun_lemmas.contains(&lemma) {
                        Ok(format!("'{}' already in PN lemmas for sentence {}.", lemma, index + 1))
                    } else {
                        sent.proper_noun_lemmas.push(lemma.clone());
                        sent.proper_noun_lemmas.sort();
                        Ok(format!("Added '{}' to PN lemmas for sentence {}.", lemma, index + 1))
                    }
                } else {
                    Err("Index out of bounds".to_string())
                }
            }
            AppCommand::RemovePnLemma { index, lemma } => {
                if let Some(sent) = self.state.document.get_mut(index) {
                    if let Some(pos) = sent.proper_noun_lemmas.iter().position(|l| l == &lemma) {
                        sent.proper_noun_lemmas.remove(pos);
                        Ok(format!("Removed '{}' from PN lemmas for sentence {}.", lemma, index + 1))
                    } else {
                        Err(format!("'{}' not found in PN lemmas for sentence {}.", lemma, index + 1))
                    }
                } else {
                    Err("Index out of bounds".to_string())
                }
            }

            // ── Segment-level editing commands ──────────────────────────

            AppCommand::EditSegment { index, tier_id, seg_id, new_text } => {
                if let Some(sent) = self.state.document.get_mut(index) {
                    if let Some(tier) = sent.tiers.get_mut(&tier_id) {
                        if let Some(seg) = tier.segments.iter_mut().find(|s| s.id == seg_id) {
                            seg.stream = crate::domain::token_stream::TokenStream::new(&new_text);
                            seg.lemmas.clear(); // will be repopulated on lemmatize/validate
                        } else {
                            return Err(format!("Segment '{}' not found in tier '{}'.", seg_id, tier_id));
                        }
                        tier.ensure_inter_segment_spacing();
                        tier.state = crate::domain::tier::TierState::Dirty;
                    } else {
                        return Err(format!("Tier '{}' not found.", tier_id));
                    }
                    sent.propagate_stale_from(&tier_id);
                    Ok(format!("Updated segment {} in tier {} for sentence {}.", seg_id, tier_id, index + 1))
                } else {
                    Err("Index out of bounds".to_string())
                }
            }

            AppCommand::AddSegment { index, tier_id, after_seg_id, new_text } => {
                if let Some(sent) = self.state.document.get_mut(index) {
                    if let Some(tier) = sent.tiers.get_mut(&tier_id) {
                        let insert_pos = tier.segments.iter().position(|s| s.id == after_seg_id)
                            .map(|p| p + 1)
                            .unwrap_or(tier.segments.len());
                        // Generate next segment id
                        let max_num: usize = tier.segments.iter()
                            .filter_map(|s| s.id.trim_start_matches(char::is_alphabetic).parse::<usize>().ok())
                            .max()
                            .unwrap_or(0);
                        let new_id = format!("S{}", max_num + 1);
                        let seg = crate::domain::segment::Segment::new(new_id.clone(), &new_text, vec![]);
                        tier.segments.insert(insert_pos, seg);
                        tier.ensure_inter_segment_spacing();
                        tier.state = crate::domain::tier::TierState::Dirty;
                        sent.propagate_stale_from(&tier_id);
                        Ok(format!("Added segment {} after {} in tier {} for sentence {}.", new_id, after_seg_id, tier_id, index + 1))
                    } else {
                        Err(format!("Tier '{}' not found.", tier_id))
                    }
                } else {
                    Err("Index out of bounds".to_string())
                }
            }

            AppCommand::RemoveSegment { index, tier_id, seg_id } => {
                if let Some(sent) = self.state.document.get_mut(index) {
                    if let Some(tier) = sent.tiers.get_mut(&tier_id) {
                        if tier.segments.len() <= 1 {
                            return Err("Cannot remove the last segment.".to_string());
                        }
                        if let Some(pos) = tier.segments.iter().position(|s| s.id == seg_id) {
                            tier.segments.remove(pos);
                            tier.state = crate::domain::tier::TierState::Dirty;
                            sent.propagate_stale_from(&tier_id);
                            Ok(format!("Removed segment {} from tier {} for sentence {}.", seg_id, tier_id, index + 1))
                        } else {
                            Err(format!("Segment '{}' not found in tier '{}'.", seg_id, tier_id))
                        }
                    } else {
                        Err(format!("Tier '{}' not found.", tier_id))
                    }
                } else {
                    Err("Index out of bounds".to_string())
                }
            }

            AppCommand::LemmatizeTier { index, tier_id } => {
                let bridge = self.state.bridge.as_ref()
                    .ok_or_else(|| "Python bridge not available.".to_string())?;
                let target_lang = self.state.project_languages.1.clone();

                let sent = self.state.document.get_mut(index)
                    .ok_or_else(|| "Index out of bounds".to_string())?;
                let tier = sent.tiers.get_mut(&tier_id)
                    .ok_or_else(|| format!("Tier '{}' not found.", tier_id))?;

                let mut output_lines: Vec<String> = Vec::new();

                for seg in tier.segments.iter_mut() {
                    let text = seg.full_text();
                    let raw_tokens = bridge.tokenize(&text, &target_lang)
                        .map_err(|e| format!("Tokenize failed for segment {}: {}", seg.id, e))?;

                    // Update existing TokenStream in-place (preserves phrasal segments)
                    seg.stream.update_lemmas_from_spacy(raw_tokens)
                        .map_err(|e| format!("Lemma update failed for segment {}: {}", seg.id, e))?;

                    // Extract lemmas list for ranking
                    seg.lemmas = seg.stream.tokens().iter()
                        .filter_map(|t| match t {
                            crate::domain::token_stream::Token::Word(wd) => Some(wd.lemmas.clone()),
                            _ => None,
                        })
                        .flatten()
                        .collect();

                    // Build display string with ranks
                    let display: Vec<String> = seg.lemmas.iter().map(|l| {
                        match crate::simulation::frequency_manager::get_rank_for_lemma(l) {
                            Some(r) => format!("{} <{}>", l, r),
                            None => format!("{} <?>", l),
                        }
                    }).collect();

                    output_lines.push(format!("[{}] {}", seg.id, display.join(", ")));
                }

                // Also rebuild tier-level lemmas from all segments
                tier.lemmas = tier.segments.iter()
                    .flat_map(|s| s.lemmas.clone())
                    .collect();
                tier.lemmas.sort();
                tier.lemmas.dedup();

                Ok(format!("Lemmatized tier {} for sentence {}:\n{}", tier_id, index + 1, output_lines.join("\n")))
            }

            AppCommand::ValidateTier { index, tier_id } => {
                // Lemmatize in-place (preserves token boundaries), then mark Valid.
                let bridge = self.state.bridge.as_ref()
                    .ok_or_else(|| "Python bridge not available.".to_string())?;
                let source_lang = self.state.project_languages.0.clone();
                let target_lang = self.state.project_languages.1.clone();
                let tier_lang = if tier_id == "base" || tier_id == "basic_base" {
                    &source_lang
                } else {
                    &target_lang
                };

                let sent = self.state.document.get_mut(index)
                    .ok_or_else(|| "Index out of bounds".to_string())?;

                // Phase 1: Lemmatize tier segments (skip for base-language tiers — no need for English lemmas)
                lemmatize_tier_segments(sent, &tier_id, bridge, tier_lang)?;

                // Phase 1b: Lemmatize mapping target texts for L1 tiers
                lemmatize_mapping_targets(sent, &tier_id, bridge, &source_lang, &target_lang)?;

                // Phase 2: DRC checks (immutable access to sent)
                let tier_violations = drc_tier(sent, &tier_id, index + 1);
                if !tier_violations.is_empty() {
                    return Err(format!("DRC failed for tier '{}' on sentence {}:\n  {}", tier_id, index + 1, tier_violations.join("\n  ")));
                }
                let other_tiers_valid = crate::domain::sentence::Sentence::WEAVE_TIERS.iter()
                    .filter(|&&t| t != tier_id)
                    .all(|&t| sent.tiers.get(t).map_or(false, |tier| tier.state == crate::domain::tier::TierState::Valid));
                if other_tiers_valid {
                    let sent_violations = drc_sentence(sent, index + 1);
                    if !sent_violations.is_empty() {
                        return Err(format!("Sentence-level DRC failed for sentence {}:\n  {}", index + 1, sent_violations.join("\n  ")));
                    }
                }

                // Phase 3: Mark Valid
                let tier = sent.tiers.get_mut(&tier_id).unwrap();
                tier.state = crate::domain::tier::TierState::Valid;
                sent.propagate_stale_from(&tier_id);

                Ok(format!("Validated tier {} for sentence {}. Tier marked Valid.", tier_id, index + 1))
            }

            // ── Edit full tier text using selected sentence + tier ───────

            AppCommand::EditText { new_text } => {
                let sent_idx = self.state.selected_sentence_idx;
                let tier_id = self.state.selected_tier_id.clone();
                let sent = self.state.document.get_mut(sent_idx)
                    .ok_or_else(|| "No sentence selected".to_string())?;
                // Use update_tier_text which marks Dirty and propagates stale
                sent.update_tier_text(&tier_id, new_text.clone());
                Ok(format!(
                    "Updated sentence {} tier '{}': {}",
                    sent_idx + 1,
                    tier_id,
                    if new_text.len() > 80 { format!("{}...", &new_text[..80]) } else { new_text }
                ))
            }

            // ── Token-level mapping commands (Bas B / Bas T) ────────────

            AppCommand::SelectTier { tier_id } => {
                use crate::app::state::{DetailView, TierView};
                const VALID_TIERS: &[&str] = &["base", "advanced_target", "moderate_target", "basic_target", "basic_base"];
                if !VALID_TIERS.contains(&tier_id.as_str()) {
                    return Err(format!(
                        "'{}' is not a valid tier. Options: base, adv, mod, bas_t, bas_b",
                        tier_id
                    ));
                }
                self.state.selected_tier_id = tier_id.clone();
                // Sync the GUI detail panel to show this tier
                match tier_id.as_str() {
                    "base" => self.state.right_view = DetailView::Tier(TierView::Base),
                    "advanced_target" => self.state.right_view = DetailView::Tier(TierView::AdvancedTarget),
                    "moderate_target" => self.state.right_view = DetailView::Tier(TierView::ModerateTarget),
                    "basic_target" => self.state.right_view = DetailView::Tier(TierView::BasicTarget),
                    "basic_base" => self.state.right_view = DetailView::Tier(TierView::BasicBase),
                    _ => {}
                }
                Ok(format!("Selected tier: {}", tier_id))
            }

            AppCommand::SplitToken { word_index } => {
                let sent_idx = self.state.selected_sentence_idx;
                let tier_id = self.state.selected_tier_id.clone();
                let sent = self.state.document.get_mut(sent_idx)
                    .ok_or_else(|| "No sentence selected".to_string())?;
                let tier = sent.tiers.get_mut(&tier_id)
                    .ok_or_else(|| format!("Tier '{}' not found.", tier_id))?;

                // Operate on the tier's concatenated token stream across all segments.
                // For simplicity, operate on the first segment (basic tiers typically have one).
                let seg = tier.segments.first_mut()
                    .ok_or_else(|| "Tier has no segments".to_string())?;

                // Before splitting, find the WordId so we can relocate the mapping.
                let old_word_id = seg.stream.word_at_one_based(word_index)
                    .map(|(_, w)| w.id)
                    .ok_or_else(|| format!("Word index {} not found", word_index))?;

                let new_ids = seg.stream.split_word_at(word_index)?;
                if new_ids.is_empty() {
                    return Ok(format!("Word {} is already atomic — nothing to split.", word_index));
                }

                // Remap: assign the old mapping entry to the first new word.
                let first_new_id = new_ids[0];
                for mapping in sent.mappings.iter_mut() {
                    if (mapping.from_tier_id == tier_id) || (mapping.to_tier_id == tier_id && mapping.from_tier_id == tier_id) {
                        for entry in mapping.entries.iter_mut() {
                            if entry.source_word_id == old_word_id {
                                entry.source_word_id = first_new_id;
                            }
                        }
                    }
                }

                // Also remap if the tier is the source
                for mapping in sent.mappings.iter_mut() {
                    if mapping.from_tier_id == tier_id {
                        for entry in mapping.entries.iter_mut() {
                            if entry.source_word_id == old_word_id {
                                entry.source_word_id = first_new_id;
                            }
                        }
                    }
                }

                // Mark tier dirty after split
                if let Some(tier) = sent.tiers.get_mut(&tier_id) {
                    tier.state = crate::domain::tier::TierState::Dirty;
                }
                sent.propagate_stale_from(&tier_id);

                Ok(format!("Split word {} into {} sub-tokens.", word_index, new_ids.len()))
            }

            AppCommand::MergeTokens { word_start, word_end } => {
                let sent_idx = self.state.selected_sentence_idx;
                let tier_id = self.state.selected_tier_id.clone();
                let sent = self.state.document.get_mut(sent_idx)
                    .ok_or_else(|| "No sentence selected".to_string())?;
                let tier = sent.tiers.get_mut(&tier_id)
                    .ok_or_else(|| format!("Tier '{}' not found.", tier_id))?;
                let seg = tier.segments.first_mut()
                    .ok_or_else(|| "Tier has no segments".to_string())?;

                // Collect WordIds that will be merged (so we can consolidate mappings)
                let merging_ids: Vec<crate::domain::primitives::WordId> = seg.stream.words_enumerated()
                    .iter()
                    .filter(|(one_idx, _, _)| *one_idx >= word_start && *one_idx <= word_end)
                    .map(|(_, _, w)| w.id)
                    .collect();

                let merged_id = seg.stream.merge_words(word_start, word_end)?;

                // Consolidate mapping entries: merge target texts of merged words
                for mapping in sent.mappings.iter_mut() {
                    if mapping.from_tier_id == tier_id {
                        let mut merged_target = String::new();
                        let mut merged_lemmas: Vec<String> = Vec::new();
                        let mut found_any = false;
                        for entry in mapping.entries.iter() {
                            if merging_ids.contains(&entry.source_word_id) {
                                if !merged_target.is_empty() && !entry.target_text.is_empty() {
                                    merged_target.push(' ');
                                }
                                merged_target.push_str(&entry.target_text);
                                merged_lemmas.extend(entry.target_lemmas.clone());
                                found_any = true;
                            }
                        }
                        // Remove all old entries for merged ids
                        mapping.entries.retain(|e| !merging_ids.contains(&e.source_word_id));
                        // Add consolidated entry
                        if found_any {
                            let mut new_entry = crate::domain::mapping::MappingEntry::new(
                                merged_id, merged_target, merged_lemmas,
                            );
                            new_entry.is_viable = true;
                            mapping.entries.push(new_entry);
                        }
                    }
                }

                // Mark tier dirty after merge
                if let Some(tier) = sent.tiers.get_mut(&tier_id) {
                    tier.state = crate::domain::tier::TierState::Dirty;
                }
                sent.propagate_stale_from(&tier_id);

                Ok(format!("Merged words {}-{} into one token.", word_start, word_end))
            }

            AppCommand::InsertToken { word_index } => {
                let sent_idx = self.state.selected_sentence_idx;
                let tier_id = self.state.selected_tier_id.clone();
                let sent = self.state.document.get_mut(sent_idx)
                    .ok_or_else(|| "No sentence selected".to_string())?;
                let tier = sent.tiers.get_mut(&tier_id)
                    .ok_or_else(|| format!("Tier '{}' not found.", tier_id))?;
                let seg = tier.segments.first_mut()
                    .ok_or_else(|| "Tier has no segments".to_string())?;

                let new_id = seg.stream.insert_word_at(word_index)?;
                // Set placeholder text so the word is visible and editable
                seg.stream.modify_word_text(new_id, "x".to_string()).ok();

                // Auto-create a mapping entry with target "x"
                let (source_tier, target_tier) = if tier_id == "basic_base" {
                    ("basic_base", "basic_target")
                } else if tier_id == "basic_target" {
                    ("basic_target", "basic_base")
                } else {
                    (&*tier_id, &*tier_id) // fallback — no mapping created
                };
                if source_tier != target_tier {
                    if let Some(mapping) = sent.mappings.iter_mut()
                        .find(|m| m.from_tier_id == source_tier && m.to_tier_id == target_tier)
                    {
                        mapping.entries.push(crate::domain::mapping::MappingEntry::new(
                            new_id, "x".to_string(), vec![],
                        ));
                    }
                }

                // Mark tier dirty after insert
                if let Some(tier) = sent.tiers.get_mut(&tier_id) {
                    tier.state = crate::domain::tier::TierState::Dirty;
                }
                sent.propagate_stale_from(&tier_id);

                Ok(format!("Inserted word 'x' at position {} with target 'x'.", word_index))
            }

            AppCommand::DeleteToken { word_index } => {
                let sent_idx = self.state.selected_sentence_idx;
                let tier_id = self.state.selected_tier_id.clone();
                let sent = self.state.document.get_mut(sent_idx)
                    .ok_or_else(|| "No sentence selected".to_string())?;
                let tier = sent.tiers.get_mut(&tier_id)
                    .ok_or_else(|| format!("Tier '{}' not found.", tier_id))?;
                let seg = tier.segments.first_mut()
                    .ok_or_else(|| "Tier has no segments".to_string())?;

                let word_id = seg.stream.word_at_one_based(word_index)
                    .map(|(_, w)| w.id)
                    .ok_or_else(|| format!("Word index {} not found", word_index))?;

                seg.stream.delete_word(word_id)?;

                // Remove mapping entries for the deleted word
                for mapping in sent.mappings.iter_mut() {
                    if mapping.from_tier_id == tier_id {
                        mapping.remove_entries_for_word(word_id);
                    }
                }

                // Mark tier dirty after delete
                if let Some(tier) = sent.tiers.get_mut(&tier_id) {
                    tier.state = crate::domain::tier::TierState::Dirty;
                }
                sent.propagate_stale_from(&tier_id);

                Ok(format!("Deleted word at position {}.", word_index))
            }

            AppCommand::EditBackground { word_index, new_text } => {
                let sent_idx = self.state.selected_sentence_idx;
                let tier_id = self.state.selected_tier_id.clone();
                let sent = self.state.document.get_mut(sent_idx)
                    .ok_or_else(|| "No sentence selected".to_string())?;
                let tier = sent.tiers.get_mut(&tier_id)
                    .ok_or_else(|| format!("Tier '{}' not found.", tier_id))?;
                let seg = tier.segments.first_mut()
                    .ok_or_else(|| "Tier has no segments".to_string())?;

                seg.stream.set_background_before_word(word_index, new_text.clone())?;

                // Mark tier dirty after background edit
                let _ = seg;
                tier.state = crate::domain::tier::TierState::Dirty;
                sent.propagate_stale_from(&tier_id);

                Ok(format!("Set background before word {} to {:?}.", word_index, new_text))
            }

            AppCommand::EditWord { word_index, new_text } => {
                let sent_idx = self.state.selected_sentence_idx;
                let tier_id = self.state.selected_tier_id.clone();
                let sent = self.state.document.get_mut(sent_idx)
                    .ok_or_else(|| "No sentence selected".to_string())?;
                let tier = sent.tiers.get_mut(&tier_id)
                    .ok_or_else(|| format!("Tier '{}' not found.", tier_id))?;
                let seg = tier.segments.first_mut()
                    .ok_or_else(|| "Tier has no segments".to_string())?;

                let word_id = seg.stream.word_at_one_based(word_index)
                    .map(|(_, w)| w.id)
                    .ok_or_else(|| format!("Word index {} not found", word_index))?;

                seg.stream.modify_word_text(word_id, new_text.clone())?;

                let _ = seg;
                tier.state = crate::domain::tier::TierState::Dirty;
                sent.propagate_stale_from(&tier_id);

                Ok(format!("Set word {} text to {:?}.", word_index, new_text))
            }

            AppCommand::EditTarget { word_index, new_text } => {
                let sent_idx = self.state.selected_sentence_idx;
                let tier_id = self.state.selected_tier_id.clone();
                let sent = self.state.document.get_mut(sent_idx)
                    .ok_or_else(|| "No sentence selected".to_string())?;
                let tier = sent.tiers.get(&tier_id)
                    .ok_or_else(|| format!("Tier '{}' not found.", tier_id))?;
                let seg = tier.segments.first()
                    .ok_or_else(|| "Tier has no segments".to_string())?;

                let word_id = seg.stream.word_at_one_based(word_index)
                    .map(|(_, w)| w.id)
                    .ok_or_else(|| format!("Word index {} not found", word_index))?;

                // Auto-init mapping if none exists
                let (source_tier, target_tier) = if tier_id == "basic_base" {
                    ("basic_base".to_string(), "basic_target".to_string())
                } else if tier_id == "basic_target" {
                    ("basic_target".to_string(), "basic_base".to_string())
                } else {
                    return Err(format!("edit_target only applies to basic_base or basic_target, not '{}'", tier_id));
                };
                if !sent.mappings.iter().any(|m| m.from_tier_id == source_tier && m.to_tier_id == target_tier) {
                    sent.add_mapping(crate::domain::mapping::TierMapping::new(source_tier.clone(), target_tier.clone()));
                }

                // Find the mapping where this tier is the source
                let mapping = sent.mappings.iter_mut()
                    .find(|m| m.from_tier_id == source_tier && m.to_tier_id == target_tier)
                    .ok_or_else(|| format!("No mapping found with source tier '{}'.", tier_id))?;

                if let Some(entry) = mapping.entries.iter_mut().find(|e| e.source_word_id == word_id) {
                    entry.target_text = new_text.clone();
                    entry.target_lemmas.clear(); // will need re-lemmatization
                } else {
                    // Create a new entry
                    mapping.entries.push(crate::domain::mapping::MappingEntry::new(
                        word_id, new_text.clone(), vec![],
                    ));
                }

                // Mark tier dirty after target edit
                if let Some(tier) = sent.tiers.get_mut(&tier_id) {
                    tier.state = crate::domain::tier::TierState::Dirty;
                }
                sent.propagate_stale_from(&tier_id);

                Ok(format!("Set target for word {} to {:?}.", word_index, new_text))
            }

            AppCommand::EditTargets { pairs } => {
                let sent_idx = self.state.selected_sentence_idx;
                let tier_id = self.state.selected_tier_id.clone();
                let sent = self.state.document.get_mut(sent_idx)
                    .ok_or_else(|| "No sentence selected".to_string())?;
                let tier = sent.tiers.get(&tier_id)
                    .ok_or_else(|| format!("Tier '{}' not found.", tier_id))?;
                let seg = tier.segments.first()
                    .ok_or_else(|| "Tier has no segments".to_string())?;

                // Resolve all word indices first
                let mut resolved: Vec<(crate::domain::primitives::WordId, usize, String)> = Vec::new();
                for (word_index, text) in &pairs {
                    let word_id = seg.stream.word_at_one_based(*word_index)
                        .map(|(_, w)| w.id)
                        .ok_or_else(|| format!("Word index {} not found", word_index))?;
                    resolved.push((word_id, *word_index, text.clone()));
                }

                // Auto-init mapping if none exists
                let (source_tier, target_tier) = if tier_id == "basic_base" {
                    ("basic_base".to_string(), "basic_target".to_string())
                } else if tier_id == "basic_target" {
                    ("basic_target".to_string(), "basic_base".to_string())
                } else {
                    return Err(format!("edit_targets only applies to basic_base or basic_target, not '{}'", tier_id));
                };
                if !sent.mappings.iter().any(|m| m.from_tier_id == source_tier && m.to_tier_id == target_tier) {
                    sent.add_mapping(crate::domain::mapping::TierMapping::new(source_tier.clone(), target_tier.clone()));
                }

                let mapping = sent.mappings.iter_mut()
                    .find(|m| m.from_tier_id == source_tier && m.to_tier_id == target_tier)
                    .ok_or_else(|| format!("No mapping found with source tier '{}'.", tier_id))?;

                let mut count = 0;
                for (word_id, _word_index, text) in &resolved {
                    if let Some(entry) = mapping.entries.iter_mut().find(|e| e.source_word_id == *word_id) {
                        entry.target_text = text.clone();
                        entry.target_lemmas.clear();
                    } else {
                        mapping.entries.push(crate::domain::mapping::MappingEntry::new(
                            *word_id, text.clone(), vec![],
                        ));
                    }
                    count += 1;
                }

                // Mark tier dirty after targets edit
                if let Some(tier) = sent.tiers.get_mut(&tier_id) {
                    tier.state = crate::domain::tier::TierState::Dirty;
                }
                sent.propagate_stale_from(&tier_id);

                Ok(format!("Set {} mapping target(s).", count))
            }

            AppCommand::AcceptMap => {
                let sent_idx = self.state.selected_sentence_idx;
                let tier_id = self.state.selected_tier_id.clone();

                // Determine the mapping direction based on the selected tier
                let (source_tier, target_tier) = if tier_id == "basic_base" {
                    ("basic_base", "basic_target")
                } else if tier_id == "basic_target" {
                    ("basic_target", "basic_base")
                } else {
                    return Err(format!("accept map only applies to basic_base or basic_target, not '{}'", tier_id));
                };

                let sent = self.state.document.get_mut(sent_idx)
                    .ok_or_else(|| "No sentence selected".to_string())?;

                // Check that all word tokens have a mapping entry
                let seg = sent.tiers.get(&tier_id)
                    .and_then(|t| t.segments.first())
                    .ok_or_else(|| "Tier/segment not found".to_string())?;

                let word_ids: Vec<crate::domain::primitives::WordId> = seg.stream.words_enumerated()
                    .iter()
                    .map(|(_, _, w)| w.id)
                    .collect();

                let mapping = sent.mappings.iter()
                    .find(|m| m.from_tier_id == source_tier && m.to_tier_id == target_tier);

                let mapped_ids: std::collections::HashSet<crate::domain::primitives::WordId> = mapping
                    .map(|m| m.entries.iter().map(|e| e.source_word_id).collect())
                    .unwrap_or_default();

                let unmapped: Vec<usize> = word_ids.iter().enumerate()
                    .filter(|(_, wid)| !mapped_ids.contains(wid))
                    .map(|(i, _)| i + 1)
                    .collect();

                if !unmapped.is_empty() {
                    return Err(format!("Cannot accept: words at indices {:?} have no mapping target.", unmapped));
                }

                // Mark the tier as Valid
                if let Some(tier) = sent.tiers.get_mut(&tier_id) {
                    tier.state = crate::domain::tier::TierState::Valid;
                }

                Ok(format!("Mapping for {} accepted. Tier marked Valid.", tier_id))
            }

            AppCommand::AcceptMapRange { start, end } => {
                let tier_id = self.state.selected_tier_id.clone();

                let (source_tier, target_tier) = if tier_id == "basic_base" {
                    ("basic_base", "basic_target")
                } else if tier_id == "basic_target" {
                    ("basic_target", "basic_base")
                } else {
                    return Err(format!("accept map only applies to basic_base or basic_target, not '{}'", tier_id));
                };

                if start < 1 || end < start {
                    return Err(format!("Invalid range: {} to {}", start, end));
                }
                let start_idx = start - 1; // convert to 0-based
                let end_idx = end.min(self.state.document.len()); // clamp

                let mut accepted = 0usize;
                let mut skipped = 0usize;
                let mut errors = Vec::new();

                for idx in start_idx..end_idx {
                    let sent = match self.state.document.get(idx) {
                        Some(s) => s,
                        None => continue,
                    };

                    // Only process sentences where the tier is Stale
                    let tier_state = sent.tiers.get(&tier_id)
                        .map(|t| t.state)
                        .unwrap_or(crate::domain::tier::TierState::Valid);
                    if tier_state != crate::domain::tier::TierState::Stale {
                        skipped += 1;
                        continue;
                    }

                    // Check that all word tokens have a mapping entry
                    let word_ids: Vec<crate::domain::primitives::WordId> = sent.tiers.get(&tier_id)
                        .and_then(|t| t.segments.first())
                        .map(|seg| {
                            seg.stream.words_enumerated()
                                .iter()
                                .map(|(_, _, w)| w.id)
                                .collect()
                        })
                        .unwrap_or_default();

                    let mapped_ids: std::collections::HashSet<crate::domain::primitives::WordId> = sent.mappings.iter()
                        .find(|m| m.from_tier_id == source_tier && m.to_tier_id == target_tier)
                        .map(|m| m.entries.iter().map(|e| e.source_word_id).collect())
                        .unwrap_or_default();

                    let unmapped: Vec<usize> = word_ids.iter().enumerate()
                        .filter(|(_, wid)| !mapped_ids.contains(wid))
                        .map(|(i, _)| i + 1)
                        .collect();

                    if !unmapped.is_empty() {
                        errors.push(format!("S{}: unmapped words {:?}", idx + 1, unmapped));
                        continue;
                    }

                    // Mark as Valid
                    if let Some(sent) = self.state.document.get_mut(idx) {
                        if let Some(tier) = sent.tiers.get_mut(&tier_id) {
                            tier.state = crate::domain::tier::TierState::Valid;
                            accepted += 1;
                        }
                    }
                }

                let mut result = format!("Bulk accept map ({} tier): {} accepted, {} skipped (not stale).",
                    tier_id, accepted, skipped);
                if !errors.is_empty() {
                    let shown = errors.len().min(5);
                    result.push_str(&format!("\n{} errors (showing first {}):", errors.len(), shown));
                    for e in &errors[..shown] {
                        result.push_str(&format!("\n  {}", e));
                    }
                }
                Ok(result)
            }

            AppCommand::InitMapping => {
                use crate::domain::mapping::TierMapping;

                let sent_idx = self.state.selected_sentence_idx;
                let tier_id = self.state.selected_tier_id.clone();

                let (source_tier, target_tier) = if tier_id == "basic_base" {
                    ("basic_base".to_string(), "basic_target".to_string())
                } else if tier_id == "basic_target" {
                    ("basic_target".to_string(), "basic_base".to_string())
                } else {
                    return Err(format!("init mapping only applies to basic_base or basic_target, not '{}'", tier_id));
                };

                let sent = self.state.document.get_mut(sent_idx)
                    .ok_or_else(|| "No sentence selected".to_string())?;

                // Check if mapping already exists
                let exists = sent.mappings.iter().any(|m| m.from_tier_id == source_tier && m.to_tier_id == target_tier);
                if exists {
                    return Ok(format!("Mapping {} → {} already exists.", source_tier, target_tier));
                }

                let mapping = TierMapping::new(source_tier.clone(), target_tier.clone());
                sent.add_mapping(mapping);
                Ok(format!("Initialized empty mapping {} → {}.", source_tier, target_tier))
            }

            AppCommand::OpenWorkspace { path } => {
                let workspace_path = std::path::PathBuf::from(&path);

                // Create directory if it doesn't exist (allows bootstrapping a new workspace)
                if !workspace_path.exists() {
                    std::fs::create_dir_all(&workspace_path)
                        .map_err(|e| format!("Cannot create workspace directory: {e}"))?;
                }

                if !workspace_path.is_dir() {
                    return Err(format!("'{}' is not a directory", path));
                }

                let config_path = workspace_path.join("config.toml");
                let config = if config_path.exists() {
                    crate::config::load_config_from_file(config_path.to_str().unwrap_or(""))?
                } else {
                    // Scaffold a default config.toml for the new workspace
                    let mut cfg = crate::config::Config::default();
                    cfg.content_project_dir = path.clone();
                    let toml_content = toml::to_string_pretty(&cfg)
                        .map_err(|e| format!("Cannot serialise default config: {e}"))?;
                    std::fs::write(&config_path, &toml_content)
                        .map_err(|e| format!("Cannot write config.toml: {e}"))?;
                    cfg
                };

                // Bootstrap copilot/ directory with default files if it doesn't exist
                let copilot_dir = workspace_path.join("copilot");
                if !copilot_dir.exists() {
                    let _ = std::fs::create_dir_all(&copilot_dir);
                    let defaults: &[(&str, &str)] = &[
                        ("_runbook.md", include_str!("../../assets/copilot/_runbook.md")),
                        ("_goal.toml", include_str!("../../assets/copilot/_goal.toml")),
                        ("_plan.toml", include_str!("../../assets/copilot/_plan.toml")),
                        ("_journal.md", include_str!("../../assets/copilot/_journal.md")),
                    ];
                    for (name, content) in defaults {
                        let _ = std::fs::write(copilot_dir.join(name), content);
                    }
                }

                // Persist last-used workspace for auto-load on next launch
                let mut gs = crate::global_settings::GlobalSettings::load();
                gs.set_workspace(&path);
                let _ = gs.save();

                // Point the LLM logger to the workspace directory
                self.state.logger = Some(crate::services::llm_logger::LlmLogger::new(
                    std::path::PathBuf::from(&path),
                ));

                // Sync model definitions and thinking budget to the routing provider
                if let Some(llm) = &self.state.llm {
                    llm.update_models(config.models.clone());
                    llm.update_thinking_budget(config.pipeline.thinking_budget_tokens);
                }

                // Hydrate output_dir from config
                if let Some(ref out_dir) = config.output_dir {
                    let resolved = if PathBuf::from(out_dir).is_absolute() {
                        PathBuf::from(out_dir)
                    } else {
                        workspace_path.join(out_dir)
                    };
                    self.state.output_dir = Some(resolved.to_string_lossy().to_string());
                }

                self.state.config = Some(config);
                self.load_chapters();
                Ok(format!("Workspace opened: {}", path))
            }
            AppCommand::LoadProject { path } => {
                let path_buf = self.resolve_path(&path);
                // Guard against memory exhaustion from oversized project files (500 MB limit).
                const MAX_PROJECT_FILE_SIZE: u64 = 500 * 1024 * 1024;
                match fs::metadata(&path_buf) {
                    Ok(meta) if meta.len() > MAX_PROJECT_FILE_SIZE => {
                        return Err(format!(
                            "Project file is too large ({:.0} MB, limit {} MB): {}",
                            meta.len() as f64 / (1024.0 * 1024.0),
                            MAX_PROJECT_FILE_SIZE / (1024 * 1024),
                            path_buf.display()
                        ));
                    }
                    Err(e) => {
                        return Err(format!("Cannot read project file '{}': {}", path_buf.display(), e));
                    }
                    _ => {}
                }
                if let Ok(bytes) = fs::read(&path_buf) {
                    // Try JSON first (new format), fall back to bincode (legacy)
                    let deser_result = serde_json::from_slice::<AppState>(&bytes)
                        .or_else(|_| bincode::deserialize::<AppState>(&bytes));
                    if let Ok(mut loaded_state) = deser_result {
                        // Restore runtime services
                        loaded_state.bridge = self.state.bridge.clone();
                        loaded_state.llm = self.state.llm.clone();
                        loaded_state.prompts = self.state.prompts.clone();
                        loaded_state.logger = self.state.logger.clone();
                        
                        // Workspace config should NOT be overwritten by the .wvl file
                        loaded_state.config = self.state.config.clone();

                        // Default batch size from stage
                        if let Some(cfg) = &self.state.config {
                            if let Some(stage) = cfg.stages.get("GenerateBasicBase") {
                                loaded_state.llm_run_batch_size = stage.batch_size_in_items;
                            }
                        }

                        self.state = loaded_state;
                        self.current_file_path = Some(path_buf);

                        // Re-hydrate output_dir from workspace config (skipped by serde)
                        if let Some(cfg) = &self.state.config {
                            if let Some(ref out_dir) = cfg.output_dir {
                                let resolved = if PathBuf::from(out_dir).is_absolute() {
                                    PathBuf::from(out_dir)
                                } else {
                                    PathBuf::from(&cfg.content_project_dir).join(out_dir)
                                };
                                self.state.output_dir = Some(resolved.to_string_lossy().to_string());
                            }
                        }

                        if let Some(cfg) = &mut self.state.config {
                            cfg.last_project_file = Some(path.clone());
                            let config_path = PathBuf::from(&cfg.content_project_dir).join("config.toml");
                            let _ = crate::config::save_config_to_file(cfg, &config_path);
                        }

                        return Ok(format!("Loaded project from {}", path));
                    }
                    return Err("Failed to deserialize project".to_string());
                }
                Err(format!("Failed to read file {}", path))
            }
            AppCommand::SaveProject { path } => {
                let save_path = if let Some(ref p) = path {
                    self.resolve_path(p)
                } else if let Some(p) = &self.current_file_path {
                    p.clone()
                } else {
                    return Err("No path provided and no current file path".to_string());
                };

                if let Ok(bytes) = serde_json::to_vec_pretty(&self.state) {
                    if fs::write(&save_path, bytes).is_ok() {
                        self.current_file_path = Some(save_path.clone());

                        if let Some(cfg) = &mut self.state.config {
                            cfg.last_project_file = Some(save_path.to_string_lossy().to_string());
                            let config_path = PathBuf::from(&cfg.content_project_dir).join("config.toml");
                            let _ = crate::config::save_config_to_file(cfg, &config_path);
                        }

                        return Ok(format!("Saved project to {:?}", save_path));
                    }
                    return Err("Failed to write file".to_string());
                }
                Err("Failed to serialize project".to_string())
            }
            AppCommand::ImportSource { path } => {
                let resolved_path = self.resolve_path(&path);
                let content = fs::read_to_string(&resolved_path).map_err(|e| e.to_string())?;
                let sentences = source_parser::parse_source_file(&content).map_err(|e| e.to_string())?;
                
                if !sentences.is_empty() {
                    self.state.document = sentences;
                } else if let Some(bridge) = &self.state.bridge {
                    let path_buf = resolved_path.clone();
                    let book_name = path_buf.file_name().and_then(|s| s.to_str()).unwrap_or("Unnamed");
                    
                    let chap = crate::services::importer::BookImporter::import_from_text_with_service(
                        &content,
                        book_name,
                        bridge,
                    )?;
                    
                    self.state.document.clear();
                    for block in chap.content_blocks {
                        if let crate::types::json_types::JsonContentBlock::Sentence(json_sentence) = block {
                            match crate::domain::bridge::json_to_domain_sentence(&json_sentence) {
                                Ok(domain_sentence) => self.state.document.push(domain_sentence),
                                Err(e) => eprintln!("Skipping invalid sentence: {e}"),
                            }
                        }
                    }
                } else {
                    return Err("No recognizable sentence markup and Python bridge not configured.".to_string());
                }
                
                self.state.book_map = None;
                self.state.book_name = resolved_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unnamed")
                    .to_string();
                self.state.selected_sentence_idx = 0;
                self.state.selected_range = None;
                // Import is not a saved project — clear file path
                self.current_file_path = None;
                self.state.llm_followup_queue.clear();
                Ok(format!("Imported {} sentences from source", self.state.document.len()))
            }
            AppCommand::ImportJson { path } => {
                let resolved_path = self.resolve_path(&path);
                let content = fs::read_to_string(&resolved_path).map_err(|e| e.to_string())?;
                let chapter: JsonChapter = serde_json::from_str(&content).map_err(|e| e.to_string())?;
                
                self.state.document.clear();
                self.state.book_map = Some(chapter.u_level_maps.clone());
                self.state.book_name = chapter.book_meta.book_name.clone();
                self.state.project_languages = (
                    chapter.book_meta.base_language.clone(),
                    chapter.book_meta.target_language.clone(),
                );

                // Import is not a saved project — clear file path
                self.current_file_path = None;
                self.state.llm_followup_queue.clear();

                let mut error_count = 0;
                for block in chapter.content_blocks {
                    if let crate::types::json_types::JsonContentBlock::Sentence(json_sentence) = block {
                        match crate::domain::bridge::json_to_domain_sentence(&json_sentence) {
                            Ok(mut domain_sentence) => {
                                // Old-format JSON files lack a "base" (source) tier.
                                // Synthesize one from the basic_base tier's text.
                                if !domain_sentence.tiers.contains_key("base") {
                                    if let Some(basic_base) = domain_sentence.tiers.get("basic_base") {
                                        let source_text = basic_base.full_text();
                                        if !source_text.is_empty() {
                                            let mut source_tier = crate::domain::tier::Tier::new("base".to_string());
                                            let segment = crate::domain::segment::Segment::new(
                                                "S1".to_string(),
                                                &source_text,
                                                vec![],
                                            );
                                            source_tier.add_segment(segment);
                                            domain_sentence.add_tier(source_tier);
                                        }
                                    }
                                }
                                self.state.document.push(domain_sentence);
                            }
                            Err(e) => {
                                eprintln!("Skipping invalid sentence: {e}");
                                error_count += 1;
                            }
                        }
                    }
                }

                self.state.selected_sentence_idx = 0;
                self.state.selected_range = None;
                Ok(format!("Imported {} sentences from JSON ({} errors)", self.state.document.len(), error_count))
            }
            AppCommand::ExportJson { path } => {
                self.execute_export_json(&path)
            }
            AppCommand::ExportLevelMap { path } => {
                self.execute_export_level_map(&path)
            }
            AppCommand::ShowLevelMap { level } => {
                self.execute_show_level_map(level)
            }
            AppCommand::ImportLevelMap { path } => {
                self.execute_import_level_map(&path)
            }
            AppCommand::SetOutputDir { path } => {
                let dir = self.resolve_path(&path);
                if !dir.exists() {
                    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create directory '{}': {}", dir.display(), e))?;
                }
                let resolved = dir.to_string_lossy().to_string();
                self.state.output_dir = Some(resolved.clone());

                // Persist to workspace config
                if let Some(cfg) = &mut self.state.config {
                    cfg.output_dir = Some(path.clone());
                    let config_path = PathBuf::from(&cfg.content_project_dir).join("config.toml");
                    let _ = crate::config::save_config_to_file(cfg, &config_path);
                }

                Ok(format!("Output directory set to '{}'", resolved))
            }
            AppCommand::GenerateWeave {
                level,
                force,
                frontier_enabled_override,
                frontier_target_pct_override,
                frontier_seed_override,
                frontier_test_mode_override,
                frontier_familiar_lemma_exclude_count_override,
            } => {
                // Guard: all sentences must be weave-ready
                if self.state.document.is_empty() {
                    return Err("No document loaded.".to_string());
                }

                // In chapter mode, check readiness only for the selected chapter
                let chapter_range: Option<(usize, usize)> = if self.state.chapter_mode {
                    let ch_idx = self.state.selected_chapter_idx
                        .ok_or("Chapter mode is on but no chapter selected. Use 'select chapter \"<name>\"'.")?;
                    let ch = self.state.chapters.get(ch_idx)
                        .ok_or("Selected chapter index is invalid.")?;
                    Some((ch.start, ch.end))
                } else {
                    None
                };

                // Check weave readiness on the relevant sentence range
                let (check_start, check_end) = chapter_range
                    .map(|(s, e)| (s.saturating_sub(1), e.saturating_sub(1)))
                    .unwrap_or((0, self.state.document.len().saturating_sub(1)));

                let not_ready: Vec<usize> = (check_start..=check_end)
                    .filter(|&i| self.state.document.get(i).map_or(true, |s| !s.is_weave_ready()))
                    .map(|i| i + 1) // 1-based for display
                    .collect();
                if !not_ready.is_empty() {
                    let preview: String = if not_ready.len() <= 10 {
                        not_ready.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")
                    } else {
                        let first: Vec<String> = not_ready[..10].iter().map(|n| n.to_string()).collect();
                        format!("{} ... and {} more", first.join(", "), not_ready.len() - 10)
                    };
                    let scope = if let Some((cs, ce)) = chapter_range {
                        let ch_name = self.state.selected_chapter_idx
                            .and_then(|idx| self.state.chapters.get(idx))
                            .map(|ch| format!("Chapter \"{}\" (S{}-S{})", ch.name, cs, ce))
                            .unwrap_or_else(|| "Chapter".to_string());
                        ch_name
                    } else {
                        "Document".to_string()
                    };
                    return Err(format!(
                        "{} is not ready for weave output. {}/{} sentences incomplete.\nIncomplete: [{}]\nUse 'weave status' or 'report sentences incomplete' for details.",
                        scope, not_ready.len(),
                        check_end - check_start + 1, preview
                    ));
                }

                // Run DRC before generating (unless --force)
                if !force {
                    let drc_range = chapter_range.map(|(s, e)| (s - 1, e - 1));
                    let drc_violations = self.run_drc(drc_range);
                    if !drc_violations.is_empty() {
                        let count = drc_violations.len();
                        let report = drc_violations.join("\n");
                        return Err(format!(
                            "DRC FAILED — {} violation(s) found. Fix these or use 'generate_weave {} --force' to override.\n{}",
                            count, level, report
                        ));
                    }
                }

                let frontier_config = crate::corpus_generator::FrontierRunConfig {
                    enabled: frontier_enabled_override.unwrap_or(self.state.frontier_enabled),
                    target_pct: frontier_target_pct_override
                        .unwrap_or(self.state.frontier_target_pct),
                    seed: frontier_seed_override.unwrap_or(self.state.frontier_seed),
                    test_mode: frontier_test_mode_override
                        .unwrap_or(self.state.frontier_test_mode),
                    familiar_lemma_exclude_count:
                        frontier_familiar_lemma_exclude_count_override
                            .unwrap_or(self.state.frontier_familiar_lemma_exclude_count),
                };

                self.execute_generate_weave(&level, chapter_range, frontier_config)
            }
            AppCommand::Calibrate { max_level } => {
                self.execute_calibrate(max_level)
            }
            AppCommand::Drc => {
                if self.state.document.is_empty() {
                    return Err("No document loaded.".to_string());
                }
                // In chapter mode, scope DRC to selected chapter
                let drc_range: Option<(usize, usize)> = if self.state.chapter_mode {
                    self.state.selected_chapter_idx.and_then(|idx| {
                        self.state.chapters.get(idx).map(|ch| (ch.start - 1, ch.end - 1))
                    })
                } else {
                    None
                };
                let violations = self.run_drc(drc_range);
                let scope_label = if drc_range.is_some() {
                    format!("chapter ({} sentence(s))", drc_range.map(|(s,e)| e - s + 1).unwrap_or(0))
                } else {
                    format!("all {} sentence(s)", self.state.document.len())
                };
                if violations.is_empty() {
                    Ok(format!("DRC PASSED — {} clean.", scope_label))
                } else {
                    let count = violations.len();
                    let report = violations.join("\n");
                    Ok(format!("DRC FAILED — {} violation(s) in {}:\n{}", count, scope_label, report))
                }
            }
            AppCommand::DrcTier { tier_id, limit } => {
                if self.state.document.is_empty() {
                    return Err("No document loaded.".to_string());
                }
                // Validate the tier_id is a known weave tier
                use crate::domain::sentence::Sentence;
                if !Sentence::WEAVE_TIERS.contains(&tier_id.as_str()) {
                    return Err(format!(
                        "Unknown tier '{}'. Valid tiers: {}",
                        tier_id,
                        Sentence::WEAVE_TIERS.join(", ")
                    ));
                }
                // In chapter mode, scope to selected chapter
                let range: Option<(usize, usize)> = if self.state.chapter_mode {
                    self.state.selected_chapter_idx.and_then(|idx| {
                        self.state.chapters.get(idx).map(|ch| (ch.start - 1, ch.end - 1))
                    })
                } else {
                    None
                };
                let violations = self.run_drc_tier(&tier_id, range);
                let tier_alias = crate::app::terminal::tier_display_alias(&tier_id);
                let scope_label = if range.is_some() {
                    format!("chapter ({} sentence(s))", range.map(|(s,e)| e - s + 1).unwrap_or(0))
                } else {
                    format!("all {} sentence(s)", self.state.document.len())
                };
                if violations.is_empty() {
                    Ok(format!("DRC PASSED — tier '{}' clean across {}.", tier_alias, scope_label))
                } else {
                    let total = violations.len();
                    let (shown, truncated) = match limit {
                        Some(n) if n < total => (&violations[..n], true),
                        _ => (&violations[..], false),
                    };
                    let report = shown.join("\n");
                    let suffix = if truncated {
                        format!("\n  ... and {} more (use 'drc {} all' to see everything)", total - limit.unwrap_or(0), tier_alias)
                    } else {
                        String::new()
                    };
                    Ok(format!("DRC FAILED — tier '{}': {} violation(s) in {} (showing {}):\n{}{}", tier_alias, total, scope_label, shown.len(), report, suffix))
                }
            }
            AppCommand::Audit => {
                if self.state.document.is_empty() {
                    return Err("No document loaded.".to_string());
                }
                let demotions = self.run_audit();
                if demotions.is_empty() {
                    Ok(format!("Audit clean — all {} sentence(s) marked Valid are structurally sound.", self.state.document.len()))
                } else {
                    let count = demotions.len();
                    let report = demotions.join("\n");
                    Ok(format!("Audit demoted {} tier(s):\n{}", count, report))
                }
            }
            AppCommand::WeaveStatus => {
                if self.state.document.is_empty() {
                    return Ok("No document loaded.".to_string());
                }
                let total = self.state.document.len();
                let complete = self.state.document.iter().filter(|s| s.is_weave_ready()).count();
                let has_level_map = self.state.book_map.as_ref().map_or(false, |m| !m.is_empty());

                let mut out = String::new();

                // Book-level status
                let mut book_parts = Vec::new();
                if complete < total {
                    book_parts.push(format!("{}/{} sentences complete, {} remaining", complete, total, total - complete));
                }
                if !has_level_map {
                    book_parts.push("no level map (run 'calibrate' to generate)".to_string());
                }
                if book_parts.is_empty() {
                    out.push_str(&format!("Book: Ready — all {} sentences are weave-complete and level map is loaded.", total));
                } else {
                    out.push_str(&format!("Book: Not Ready — {}", book_parts.join("; ")));
                }

                // Chapter-level status
                if !self.state.chapters.is_empty() {
                    out.push_str("\n\nChapters:");
                    for (ci, ch) in self.state.chapters.iter().enumerate() {
                        let s0 = ch.start.saturating_sub(1);
                        let e0 = ch.end.saturating_sub(1);
                        let ch_total = e0 - s0 + 1;
                        let ch_complete = (s0..=e0)
                            .filter(|&i| self.state.document.get(i).map_or(false, |s| s.is_weave_ready()))
                            .count();
                        let selected = self.state.selected_chapter_idx == Some(ci);
                        let marker = if selected { " ←" } else { "" };
                        let ch_status = if ch_complete == ch_total && has_level_map {
                            "Ready".to_string()
                        } else if ch_complete == ch_total {
                            "Complete (needs calibration)".to_string()
                        } else {
                            format!("{}/{} complete", ch_complete, ch_total)
                        };
                        out.push_str(&format!("\n  [{}] \"{}\" (S{}-S{}): {}{}", ci + 1, ch.name, ch.start, ch.end, ch_status, marker));
                    }
                }

                Ok(out)
            }
            AppCommand::ReportSentencesIncomplete { limit } => {
                if self.state.document.is_empty() {
                    return Ok("No document loaded.".to_string());
                }
                let incomplete: Vec<String> = self.state.document.iter().enumerate()
                    .filter(|(_, s)| !s.is_weave_ready())
                    .map(|(i, s)| {
                        use crate::domain::tier::TierState;
                        let mut issues: Vec<String> = Vec::new();
                        for &tid in crate::domain::sentence::Sentence::WEAVE_TIERS {
                            match s.tiers.get(tid) {
                                None => issues.push(format!("{}: missing", tid)),
                                Some(t) if t.state != TierState::Valid => {
                                    let label = match t.state {
                                        TierState::Dirty => "dirty",
                                        TierState::Stale => "stale",
                                        TierState::Pending => "pending",
                                        TierState::Broken => "BROKEN",
                                        TierState::Valid => unreachable!(),
                                    };
                                    issues.push(format!("{}: {}", tid, label));
                                }
                                _ => {}
                            }
                        }
                        if !s.has_diglot_mapping() {
                            issues.push("fwd_mapping: missing".to_string());
                        }
                        if !s.has_inverse_diglot_mapping() {
                            issues.push("inv_mapping: missing".to_string());
                        }
                        let detail = if issues.is_empty() { String::new() } else { format!(" [{}]", issues.join(", ")) };
                        format!("  {} (sentence {}){}", s.id, i + 1, detail)
                    })
                    .collect();
                if incomplete.is_empty() {
                    Ok("All sentences are weave-complete!".to_string())
                } else {
                    let total = incomplete.len();
                    let shown = match limit {
                        Some(n) => &incomplete[..n.min(total)],
                        None => &incomplete,
                    };
                    let suffix = if limit.is_some() && total > limit.unwrap_or(0) {
                        format!("\n  ... and {} more (use 'report sentences incomplete' for full list)", total - limit.unwrap_or(0))
                    } else {
                        String::new()
                    };
                    Ok(format!("{} incomplete sentence(s):\n{}{}", total, shown.join("\n"), suffix))
                }
            }
            AppCommand::ReportSentencesComplete { limit } => {
                if self.state.document.is_empty() {
                    return Ok("No document loaded.".to_string());
                }
                let complete: Vec<String> = self.state.document.iter().enumerate()
                    .filter(|(_, s)| s.is_weave_ready())
                    .map(|(i, s)| format!("  {} (sentence {})", s.id, i + 1))
                    .collect();
                if complete.is_empty() {
                    Ok("No sentences are weave-complete yet.".to_string())
                } else {
                    let total = complete.len();
                    let shown = match limit {
                        Some(n) => &complete[..n.min(total)],
                        None => &complete,
                    };
                    let suffix = if limit.is_some() && total > limit.unwrap_or(0) {
                        format!("\n  ... and {} more (use 'report sentences complete' for full list)", total - limit.unwrap_or(0))
                    } else {
                        String::new()
                    };
                    Ok(format!("{} complete sentence(s):\n{}{}", total, shown.join("\n"), suffix))
                }
            }
            AppCommand::ReportSentence { start_index, end_index } => {
                if self.state.document.is_empty() {
                    return Err("No document loaded.".to_string());
                }
                let max_idx = self.state.document.len().saturating_sub(1);
                let s = start_index.min(max_idx);
                let e = end_index.min(max_idx);
                let (s, e) = if s <= e { (s, e) } else { (e, s) };

                let tier_labels: &[(&str, &str)] = &[
                    ("base",             "Base (Source)"),
                    ("basic_base",       "Basic Base"),
                    ("advanced_target",  "Advanced Target"),
                    ("moderate_target",  "Moderate Target"),
                    ("basic_target",     "Basic Target"),
                ];

                let mut out = String::new();
                for idx in s..=e {
                    let sent = &self.state.document[idx];
                    let wc = sent.weave_completeness();
                    let wc_label = match wc {
                        crate::domain::sentence::Completeness::Complete => "READY",
                        crate::domain::sentence::Completeness::Incomplete => "INCOMPLETE",
                        crate::domain::sentence::Completeness::Empty => "EMPTY",
                    };
                    out.push_str(&format!("=== {} (sentence {}) — Weave: {} ===\n", sent.id, idx + 1, wc_label));

                    for &(tid, label) in tier_labels {
                        let status = sent.tier_status_display(tid);
                        let (preview, wc_str) = sent.get_tier(tid)
                            .map(|t| {
                                let ft = t.full_text();
                                let word_count: usize = t.segments.iter()
                                    .map(|s| s.stream.word_count())
                                    .sum();
                                let p = if ft.len() > 60 {
                                    // Find a char boundary at or before byte 57
                                    let mut end = 57.min(ft.len());
                                    while end > 0 && !ft.is_char_boundary(end) {
                                        end -= 1;
                                    }
                                    format!("\"{}...\"", &ft[..end])
                                } else {
                                    format!("\"{}\"", ft)
                                };
                                (p, format!("({} words)", word_count))
                            })
                            .unwrap_or_else(|| ("—".to_string(), String::new()));
                        out.push_str(&format!("  {:<20} {:<12} {:<12} {}\n", label, status, wc_str, preview));
                    }

                    // Mappings
                    let diglot_status = if sent.has_diglot_mapping() { "valid" } else { "empty" };
                    let inv_diglot_status = if sent.has_inverse_diglot_mapping() { "valid" } else { "empty" };
                    out.push_str(&format!("  {:<20} {}\n", "Diglot Mapping", diglot_status));
                    out.push_str(&format!("  {:<20} {}\n", "Inverse Diglot", inv_diglot_status));

                    if idx < e {
                        out.push('\n');
                    }
                }
                Ok(out)
            }
            AppCommand::ConfigSet { key, value } => {
                if let Some(config) = &mut self.state.config {
                    let result = {
                    let parts: Vec<&str> = key.split('.').collect();
                    if parts.len() == 1 {
                        match parts[0] {
                            "open_last_project" => {
                                if let Ok(v) = value.parse::<bool>() {
                                    config.open_last_project = Some(v);
                                    Ok(format!("Updated open_last_project to {}", v))
                                } else {
                                    Err("Invalid boolean".to_string())
                                }
                            }
                            "custom_frequency_list_path" => {
                                if value.trim().is_empty() || value == "none" {
                                    config.custom_frequency_list_path = None;
                                    Ok("Clear custom_frequency_list_path".to_string())
                                } else {
                                    config.custom_frequency_list_path = Some(value.clone());
                                    Ok(format!("Updated custom_frequency_list_path to {}", value))
                                }
                            }
                            "youtube_client_secret_file" => {
                                if value.trim().is_empty() || value == "none" {
                                    config.youtube_client_secret_file = None;
                                    Ok("Cleared youtube_client_secret_file".to_string())
                                } else {
                                    // Strip surrounding quotes if present
                                    let v = if (value.starts_with('"') && value.ends_with('"'))
                                        || (value.starts_with('\'') && value.ends_with('\''))
                                    {
                                        value[1..value.len()-1].to_string()
                                    } else {
                                        value.clone()
                                    };
                                    config.youtube_client_secret_file = Some(v.clone());
                                    Ok(format!("Updated youtube_client_secret_file to '{}'", v))
                                }
                            }
                            _ => Err(format!("Unknown root field: {}", parts[0]))
                        }
                    } else if parts.len() == 2 && parts[0] == "pipeline" {
                        let field = parts[1];
                        match field {
                            "max_api_retries" => {
                                if let Ok(v) = value.parse::<u32>() {
                                    config.pipeline.max_api_retries = v;
                                    Ok(format!("Updated pipeline.max_api_retries to {}", v))
                                } else { Err("Invalid u32".to_string()) }
                            }
                            "max_validation_retries" => {
                                if let Ok(v) = value.parse::<u32>() {
                                    config.pipeline.max_validation_retries = v;
                                    Ok(format!("Updated pipeline.max_validation_retries to {}", v))
                                } else { Err("Invalid u32".to_string()) }
                            }
                            "retry_delay" => {
                                if let Ok(v) = value.parse::<u32>() {
                                    config.pipeline.retry_delay = v;
                                    Ok(format!("Updated pipeline.retry_delay to {}", v))
                                } else { Err("Invalid u32".to_string()) }
                            }
                            "thinking_budget_tokens" => {
                                if value.trim().is_empty() || value == "none" {
                                    config.pipeline.thinking_budget_tokens = None;
                                    Ok("Cleared pipeline.thinking_budget_tokens".to_string())
                                } else if let Ok(v) = value.parse::<u32>() {
                                    config.pipeline.thinking_budget_tokens = Some(v);
                                    Ok(format!("Updated pipeline.thinking_budget_tokens to {}", v))
                                } else { Err("Invalid u32".to_string()) }
                            }
                            _ => Err(format!("Unknown pipeline field: {}", field)),
                        }
                    } else if parts.len() == 3 && parts[0] == "stages" {
                        let stage_name = parts[1];
                        let field = parts[2];
                        if let Some(stage) = config.stages.get_mut(stage_name) {
                            match field {
                                "primary_model" => {
                                    stage.primary_model = value.clone();
                                    Ok(format!("Updated {}.primary_model to {}", stage_name, value))
                                }
                                "fallback_model" => {
                                    if value.trim().is_empty() || value == "none" {
                                        stage.fallback_model = None;
                                        Ok(format!("Cleared {}.fallback_model", stage_name))
                                    } else {
                                        stage.fallback_model = Some(value.clone());
                                        Ok(format!("Updated {}.fallback_model to {}", stage_name, value))
                                    }
                                }
                                "batch_size_in_items" => {
                                    if let Ok(v) = value.parse::<usize>() {
                                        stage.batch_size_in_items = v;
                                        Ok(format!("Updated {}.batch_size_in_items to {}", stage_name, v))
                                    } else {
                                        Err("Invalid number".to_string())
                                    }
                                }
                                "thinking_budget_tokens" => {
                                    if value.trim().is_empty() || value == "none" {
                                        stage.thinking_budget_tokens = None;
                                        Ok(format!("Cleared {}.thinking_budget_tokens", stage_name))
                                    } else if let Ok(v) = value.parse::<u32>() {
                                        stage.thinking_budget_tokens = Some(v);
                                        Ok(format!("Updated {}.thinking_budget_tokens to {}", stage_name, v))
                                    } else { Err("Invalid u32".to_string()) }
                                }
                                "thinking_on_first_attempt" => {
                                    if value.trim().is_empty() || value == "none" {
                                        stage.thinking_on_first_attempt = None;
                                        Ok(format!("Cleared {}.thinking_on_first_attempt", stage_name))
                                    } else if let Ok(v) = value.parse::<bool>() {
                                        stage.thinking_on_first_attempt = Some(v);
                                        Ok(format!("Updated {}.thinking_on_first_attempt to {}", stage_name, v))
                                    } else { Err("Invalid boolean".to_string()) }
                                }
                                _ => Err(format!("Unknown stage field: {}", field)),
                            }
                        } else {
                            Err(format!("Stage '{}' not found", stage_name))
                        }
                    } else if parts.len() == 3 && parts[0] == "models" {
                        let model_name = parts[1];
                        let field = parts[2];
                        if let Some(model) = config.models.get_mut(model_name) {
                            let result = match field {
                                "provider" => {
                                    model.provider = value.clone();
                                    Ok(format!("Updated {}.provider to {}", model_name, value))
                                }
                                "name" => {
                                    model.name = value.clone();
                                    Ok(format!("Updated {}.name to {}", model_name, value))
                                }
                                "max_input_tokens" => {
                                    if let Ok(v) = value.parse::<usize>() {
                                        model.max_input_tokens = v;
                                        Ok(format!("Updated {}.max_input_tokens to {}", model_name, v))
                                    } else {
                                        Err("Invalid number".to_string())
                                    }
                                }
                                _ => Err(format!("Unknown model field: {}", field)),
                            };
                            // Sync updated model definitions to the routing provider
                            if result.is_ok() {
                                if let Some(llm) = &self.state.llm {
                                    llm.update_models(config.models.clone());
                                }
                            }
                            result
                        } else {
                            Err(format!("Model '{}' not found", model_name))
                        }
                    } else if parts.len() == 2 && parts[0] == "copilot" {
                        let field = parts[1];
                        // Ensure copilot config exists
                        if config.copilot.is_none() {
                            config.copilot = Some(crate::config::CopilotConfig {
                                model: None,
                                max_turns: Some(50),
                            });
                        }
                        let cop = config.copilot.as_mut().unwrap();
                        match field {
                            "model" => {
                                if value.trim().is_empty() || value == "none" {
                                    cop.model = None;
                                    Ok("Copilot model disabled.".to_string())
                                } else {
                                    cop.model = Some(value.clone());
                                    Ok(format!("Updated copilot.model to '{}'", value))
                                }
                            }
                            "max_turns" => {
                                if let Ok(v) = value.parse::<u32>() {
                                    cop.max_turns = Some(v);
                                    Ok(format!("Updated copilot.max_turns to {}", v))
                                } else {
                                    Err("Invalid u32".to_string())
                                }
                            }
                            _ => Err(format!("Unknown copilot field: {}", field)),
                        }
                    } else {
                        Err("Invalid key format.".to_string())
                    }
                    };
                    // Persist successful config changes to disk
                    if result.is_ok() {
                        let config_path = PathBuf::from(&config.content_project_dir).join("config.toml");
                        let _ = crate::config::save_config_to_file(config, &config_path);
                    }
                    result
                } else {
                    Err("Config not loaded".to_string())
                }
            }
            AppCommand::ConfigList => {
                 if let Some(config) = &self.state.config {
                     // Pretty print config
                     let toml_str = toml::to_string_pretty(config).map_err(|e| e.to_string())?;
                     Ok(toml_str)
                 } else {
                     Err("Config not loaded".to_string())
                 }
            }
            AppCommand::ConfigAddModel { alias } => {
                if let Some(config) = &mut self.state.config {
                    if config.models.contains_key(&alias) {
                        return Err(format!("Model alias '{}' already exists", alias));
                    }
                    config.models.insert(alias.clone(), crate::config::ModelConfig {
                        provider: String::new(),
                        name: String::new(),
                        max_input_tokens: 10000,
                    });
                    if let Some(llm) = &self.state.llm {
                        llm.update_models(config.models.clone());
                    }
                    Ok(format!("Added model '{}'", alias))
                } else {
                    Err("Config not loaded".to_string())
                }
            }
            AppCommand::ConfigRemoveModel { alias } => {
                if let Some(config) = &mut self.state.config {
                    if config.models.remove(&alias).is_some() {
                        if let Some(llm) = &self.state.llm {
                            llm.update_models(config.models.clone());
                        }
                        Ok(format!("Removed model '{}'", alias))
                    } else {
                        Err(format!("Model alias '{}' not found", alias))
                    }
                } else {
                    Err("Config not loaded".to_string())
                }
            }
            AppCommand::ConfigRenameModel { old_alias, new_alias } => {
                if let Some(config) = &mut self.state.config {
                    if !config.models.contains_key(&old_alias) {
                        return Err(format!("Model alias '{}' not found", old_alias));
                    }
                    if config.models.contains_key(&new_alias) {
                        return Err(format!("Model alias '{}' already exists", new_alias));
                    }
                    if let Some(model_cfg) = config.models.remove(&old_alias) {
                        config.models.insert(new_alias.clone(), model_cfg);
                        // Update any stages that reference the old alias
                        for stage in config.stages.values_mut() {
                            if stage.primary_model == old_alias {
                                stage.primary_model = new_alias.clone();
                            }
                            if stage.fallback_model.as_ref() == Some(&old_alias) {
                                stage.fallback_model = Some(new_alias.clone());
                            }
                        }
                        if let Some(llm) = &self.state.llm {
                            llm.update_models(config.models.clone());
                        }
                        Ok(format!("Renamed model '{}' to '{}'", old_alias, new_alias))
                    } else {
                        Err(format!("Model alias '{}' not found", old_alias))
                    }
                } else {
                    Err("Config not loaded".to_string())
                }
            }
            AppCommand::CheckStatus => {
                let bridge_status = if self.state.bridge.is_some() { "OK" } else { "OFF" };
                let llm_status = if self.state.llm.is_some() { "OK" } else { "OFF" };
                let config_status = if self.state.config.is_some() { "OK" } else { "OFF" };
                let mode_info = if self.state.chapter_mode {
                    let ch_info = self.state.selected_chapter_idx
                        .and_then(|idx| self.state.chapters.get(idx))
                        .map(|ch| format!("\"{}\" (S{}-S{})", ch.name, ch.start, ch.end))
                        .unwrap_or_else(|| "(none selected)".to_string());
                    format!("Chapter — selected: {}", ch_info)
                } else {
                    "Book".to_string()
                };
                let doc_info = if self.state.document.is_empty() {
                    "No document loaded".to_string()
                } else {
                    format!("{} sentences", self.state.document.len())
                };
                Ok(format!("Bridge: {}\nLLM: {}\nConfig: {}\nMode: {}\nDocument: {}",
                    bridge_status, llm_status, config_status, mode_info, doc_info))
            }
            AppCommand::GenerateStage { stage_name, start_index, end_index, no_followup } => {
                if self.state.llm.is_none() || self.state.prompts.is_none() || self.state.logger.is_none() {
                    return Err("LLM pipeline services not ready (prompts or logger missing)".to_string());
                }
                if self.state.config.is_none() {
                    return Err("Config not loaded".to_string());
                }
                
                let config = self.state.config.as_ref().unwrap();
                let stage_config = config.get_stage_config(&stage_name).ok_or_else(|| {
                    let available = config.stages.keys().cloned().collect::<Vec<_>>().join(", ");
                    format!(
                        "Stage '{}' not found in config. Available stages: [{}]\n\
                         You can also use tier aliases: adv, mod, bas_b, bas_t, phrase_map, inv_map",
                        stage_name, available
                    )
                })?;
                
                let batch_size = stage_config.batch_size_in_items;
                let model_alias = stage_config.primary_model.clone();
                let fallback_alias = stage_config.fallback_model.clone();

                // Validate that the model aliases exist in [models] before spawning
                let model_cfg = config.get_model_config(&model_alias).ok_or_else(|| {
                    format!(
                        "Primary model alias '{}' for stage '{}' not found in [models] section of config.toml.\n\
                         Available models: [{}]",
                        model_alias,
                        stage_name,
                        config.models.keys().cloned().collect::<Vec<_>>().join(", ")
                    )
                })?;
                let model_display = format!("{} (alias '{}', provider: {})", model_cfg.name, model_alias, model_cfg.provider);

                if let Some(ref fb) = fallback_alias {
                    if config.get_model_config(fb).is_none() {
                        return Err(format!(
                            "Fallback model alias '{}' for stage '{}' not found in [models] section of config.toml.\n\
                             Available models: [{}]",
                            fb,
                            stage_name,
                            config.models.keys().cloned().collect::<Vec<_>>().join(", ")
                        ));
                    }
                }
                
                // Map Stage Name to Prompt Name and Target Tier
                // This logic should ideally be in a domain service, but hardcoding map here for now based on context
                let (prompt_name, target_tier, source_tier) = match stage_name.as_str() {
                    "GenerateBasicBase" => ("simplify_to_basic_english", "basic_base", "base"),
                    "GenerateBasicTarget" => ("translate_text_basic", "basic_target", "basic_base"),
                    "GenerateAdvancedTarget" => ("translate_text", "advanced_target", "base"),
                    "GenerateModerateTarget" => ("simplify_segments_moderate", "moderate_target", "advanced_target"),
                    "GeneratePhraseMap" => ("generate_diglot_map", "MAPPING:basic_base:basic_target", "basic_base"),
                    "GenerateInversePhraseMap" => ("generate_inverse_phrase_map", "MAPPING:basic_target:basic_base", "basic_target"),
                    _ => return Err(format!("Unknown stage mapping for '{}'", stage_name)),
                };

                let start = std::cmp::min(start_index, self.state.document.len().saturating_sub(1));
                let end = std::cmp::min(end_index, self.state.document.len().saturating_sub(1));
                let (s, e) = if start <= end { (start, end) } else { (end, start) };

                // Mapping stages accept Pending source tiers (content valid, mapping pending)
                let is_mapping_stage = matches!(stage_name.as_str(),
                    "GeneratePhraseMap" | "GenerateInversePhraseMap");

                // ── Source tier validity check ──────────────────────────────────
                // Every sentence in the range must have its source tier present
                // and Valid.  Bail early with a clear message if not.
                for idx in s..=e {
                    if let Some(sent) = self.state.document.get(idx) {
                        match sent.get_tier(source_tier) {
                            None => {
                                return Err(format!(
                                    "Cannot run '{}' for sentences {}-{}: \
                                     source tier '{}' is missing on {} (index {}).",
                                    stage_name, s + 1, e + 1, source_tier, sent.id, idx + 1
                                ));
                            }
                            Some(tier) if tier.state != TierState::Valid
                                && !(is_mapping_stage && (tier.state == TierState::Pending || tier.state == TierState::Broken || tier.state == TierState::Dirty)) =>
                            {
                                let state_label = match tier.state {
                                    TierState::Dirty  => "Dirty (unapproved edits)",
                                    TierState::Stale  => "Stale (upstream changed)",
                                    TierState::Pending => "Pending (mapping/segmentation needed)",
                                    TierState::Broken => "Broken",
                                    TierState::Valid   => unreachable!(),
                                };
                                return Err(format!(
                                    "Cannot run '{}' for sentences {}-{}: \
                                     source tier '{}' is {} on {} (index {}). \
                                     Please fix and approve before processing.",
                                    stage_name, s + 1, e + 1, source_tier, state_label, sent.id, idx + 1
                                ));
                            }
                            _ => {} // Valid — OK
                        }
                    }
                }

                // Build items — segment-level for GenerateModerateTarget,
                // sentence-level for everything else.
                let segment_level = stage_name == "GenerateModerateTarget";
                let mut items: Vec<(usize, String, String)> = Vec::new();
                for idx in s..=e {
                    if let Some(sent) = self.state.document.get(idx) {
                        if segment_level {
                            // Emit one item per segment: (idx, "S5_S1", segment_text)
                            if let Some(tier) = sent.get_tier(source_tier) {
                                for (seg_i, seg) in tier.segments.iter().enumerate() {
                                    let seg_id = format!("{}_S{}", sent.id, seg_i + 1);
                                    let text = seg.full_text().replace("--", " — ");
                                    if !text.trim().is_empty() {
                                        items.push((idx, seg_id, text));
                                    }
                                }
                            }
                        } else {
                            let mut source_text = sent.get_tier(source_tier).map(|t| t.full_text()).unwrap_or_default();
                            // Normalise bare double-dashes (Gutenberg em-dash
                            // convention) so the LLM receives proper punctuation
                            // and doesn't produce merged tokens like "word--word".
                            source_text = source_text.replace("--", " — ");
                            // For mapping stages, strip punctuation so the LLM
                            // only sees words and cannot map ¿, ?, etc.
                            // Replace punctuation with spaces (not remove) to
                            // preserve word boundaries, e.g. "mundo--que" → "mundo que".
                            // EXCEPTION: Apostrophes and hyphens are kept
                            // because apostrophes are integral to contractions
                            // (I'm, don't) and possessives (Hugson's, Alice's),
                            // and hyphens are integral to hyphenated words
                            // (Ep-pe, twenty-one) which must stay atomic.
                            if is_mapping_stage {
                                source_text = source_text.chars()
                                    .map(|c| {
                                        if c == '\'' || c == '\u{2019}' || c == '-' {
                                            c // preserve apostrophes, right single quotes, and hyphens
                                        } else if c.is_ascii_punctuation() || matches!(c, '¿' | '¡' | '«' | '»' | '—' | '…') {
                                            ' '
                                        } else {
                                            c
                                        }
                                    })
                                    .collect::<String>()
                                    .split_whitespace()
                                    .collect::<Vec<_>>()
                                    .join(" ");
                            }
                            if !source_text.trim().is_empty() {
                                items.push((idx, sent.id.clone(), source_text));
                            }
                        }
                    }
                }

                if items.is_empty() {
                    return Ok("No items to process in range".to_string());
                }

                // ── Auto-mapping interleave for basic tier stages ────────────────
                // When generating BasicBase or BasicTarget, split the work into
                // "master batches" and interleave mapping generation after each.
                //
                // Master batch size = max(translation_batch, mapping_batch) so
                // each stage uses its own batch_size inside spawn_llm_job while
                // the scheduling ranges stay aligned.
                //
                // BasicBase  → follow-up: PhraseMap only
                // BasicTarget → follow-up: PhraseMap + InversePhraseMap
                let needs_auto_mapping = !no_followup && matches!(
                    stage_name.as_str(),
                    "GenerateBasicBase" | "GenerateBasicTarget"
                );

                if needs_auto_mapping {
                    let config_ref = self.state.config.as_ref().unwrap();

                    // Determine which mapping stages follow this translation stage
                    let mapping_stages: Vec<&str> = match stage_name.as_str() {
                        "GenerateBasicBase" => vec!["GeneratePhraseMap"],
                        "GenerateBasicTarget" => vec!["GenerateInversePhraseMap"],
                        _ => vec![],
                    };

                    // Compute master batch size = max(translation, largest follower)
                    let follower_max_batch = mapping_stages.iter()
                        .filter_map(|s| config_ref.get_stage_config(s))
                        .map(|sc| sc.batch_size_in_items)
                        .max()
                        .unwrap_or(batch_size);
                    let master_batch_size = batch_size.max(follower_max_batch);

                    // Split items into balanced master batches
                    let master_batches = crate::services::llm_worker::compute_balanced_chunks(
                        items.clone(), master_batch_size,
                    );

                    // Build the follow-up queue:
                    // For each master batch: translate (leader) → mapping stages (followers)
                    for (batch_idx, batch) in master_batches.iter().enumerate() {
                        let batch_start = batch.first().unwrap().0;
                        let batch_end = batch.last().unwrap().0;

                        // Queue the translation step for all batches AFTER the first
                        // (the first batch is spawned directly below)
                        if batch_idx > 0 {
                            self.state.llm_followup_queue.push_back(format!(
                                "run generate {} {} {}",
                                stage_name,
                                batch_start + 1,
                                batch_end + 1,
                            ));
                        }

                        // Queue only the appropriate mapping stages
                        for mapping_stage in &mapping_stages {
                            self.state.llm_followup_queue.push_back(format!(
                                "run generate {} {} {}",
                                mapping_stage,
                                batch_start + 1,
                                batch_end + 1,
                            ));
                        }
                    }

                    // Use only the first master batch for the immediate spawn
                    items = master_batches.into_iter().next().unwrap();
                }

                // Spawn Job
                let prompts = self.state.prompts.clone().unwrap();
                let llm = self.state.llm.clone().unwrap();
                let logger = self.state.logger.clone().unwrap();
                let log_file_path = logger.log_file_path().display().to_string();
                let config_obj = self.state.config.clone().unwrap();
                let (base_lang, target_lang) = self.state.project_languages.clone();

                let item_count = items.len();
                let (rx, cancel) = crate::services::llm_worker::spawn_llm_job(
                    prompts,
                    llm,
                    logger,
                    config_obj,
                    base_lang,
                    target_lang,
                    prompt_name.to_string(),
                    target_tier.to_string(),
                    items,
                    batch_size,
                    model_alias.clone(),
                    fallback_alias,
                    segment_level,
                );

                self.state.llm_results_receiver = Some(rx);
                self.state.llm_cancel_flag = Some(cancel);
                self.state.llm_job_total = item_count;
                self.state.llm_job_done = 0;
                self.state.llm_job_stage = stage_name.to_string();
                self.state.llm_job_target_tier = target_tier.to_string();
                self.state.llm_job_model = model_alias.clone();
                self.state.show_llm_run = false; // Hide UI dialog if open

                let queue_len = self.state.llm_followup_queue.len();
                let queue_info = if queue_len > 0 {
                    format!("\n  Follow-up steps queued: {}", queue_len)
                } else {
                    String::new()
                };

                Ok(format!(
                    "Started stage '{}' for {} items\n  Model: {}\n  Batch size: {}\n  LLM log: {}{}",
                    stage_name,
                    self.state.llm_job_total,
                    model_display,
                    batch_size,
                    log_file_path,
                    queue_info,
                ))
            }
            AppCommand::MeasureAvd { path } => {
                self.execute_measure_avd(&path)
            }
            AppCommand::MeasureUserScore { path } => {
                self.execute_measure_user_score(&path)
            }
            AppCommand::SetKey { provider, value } => {
                crate::services::secrets::set_key(&provider, &value)
                    .map(|_| format!("API key for '{}' stored in OS keychain.", provider))
            }
            AppCommand::DeleteKey { provider } => {
                crate::services::secrets::delete_key(&provider)
                    .map(|_| format!("API key for '{}' removed from OS keychain.", provider))
            }
            AppCommand::KeyStatus => {
                Ok(crate::services::secrets::status_report())
            }
            AppCommand::DebugDump { start_index, end_index, path } => {
                self.execute_debug_dump(start_index, end_index, path.as_deref())
            }
            // ----- AV Production commands -----
            AppCommand::AvInit => {
                self.av_execute(|producer| {
                    let _ = producer; // manifest created by av_execute
                    Ok("AV manifest initialized.".to_string())
                })
            }
            AppCommand::AvStatus => {
                self.av_execute(|producer| {
                    let statuses = producer.scan();
                    let ill_count = producer.count_illustrations();
                    let mut out = crate::services::av_producer::AvProducer::format_status_table(&statuses);
                    out.push_str(&format!("Illustrations: {} image(s) in /illustrations\n", ill_count));
                    Ok(out)
                })
            }
            AppCommand::AvMark { stems } => {
                self.av_execute_mut(|producer| {
                    let added = producer.mark(&stems)?;
                    Ok(format!("Marked {} file(s). Total marked: {}", added, producer.manifest.files.marked.len()))
                })
            }
            AppCommand::AvUnmark { stems } => {
                self.av_execute_mut(|producer| {
                    let removed = producer.unmark(&stems)?;
                    Ok(format!("Unmarked {} file(s). Total marked: {}", removed, producer.manifest.files.marked.len()))
                })
            }
            AppCommand::AvMarkAll => {
                self.av_execute_mut(|producer| {
                    let added = producer.mark_all()?;
                    Ok(format!("Marked {} file(s). Total marked: {}", added, producer.manifest.files.marked.len()))
                })
            }
            AppCommand::AvClearMarks => {
                self.av_execute_mut(|producer| {
                    let cleared = producer.clear_marks()?;
                    Ok(format!("Cleared {} mark(s).", cleared))
                })
            }
            AppCommand::AvConfigShow => {
                self.av_execute(|producer| {
                    let m = &producer.manifest;
                    let mut out = String::new();
                    out.push_str("--- TTS Config ---\n");
                    out.push_str(&format!("  service:             {}\n", m.tts.service));
                    out.push_str(&format!("  model:               {}\n", m.tts.model));
                    out.push_str(&format!("  voices:              {}\n", m.tts.voices.join(", ")));
                    out.push_str(&format!("  prompt_prefix:       {}\n", if m.tts.prompt_prefix.is_empty() { "(empty)" } else { &m.tts.prompt_prefix }));
                    out.push_str(&format!("  use_vertex_auth:     {}\n", m.tts.use_vertex_auth));
                    out.push_str(&format!("  output_format:       {}\n", m.tts.output_format));
                    out.push_str(&format!("  chunk_max_chars:     {}\n", m.tts.chunk_max_chars));
                    out.push_str(&format!("  max_api_retries:     {}\n", m.tts.max_api_retries));
                    out.push_str(&format!("  retry_delay:         {}\n", m.tts.retry_delay));
                    out.push_str(&format!("  concurrent_requests: {}\n", m.tts.concurrent_requests));
                    out.push_str("--- Video Config ---\n");
                    out.push_str(&format!("  image_duration:           {}\n", m.video.image_duration));
                    out.push_str(&format!("  frame_rate:               {}\n", m.video.frame_rate));
                    out.push_str(&format!("  max_sentences_per_video:  {}{}\n",
                        m.video.max_sentences_per_video,
                        if m.video.max_sentences_per_video == 0 { " (no limit)" } else { "" }
                    ));
                    out.push_str("--- Illustrations Config ---\n");
                    out.push_str(&format!("  style_prefix:              {}\n", m.illustrations.style_prefix));
                    out.push_str(&format!("  prompt_model:              {}\n", m.illustrations.prompt_model));
                    out.push_str(&format!("  image_model:               {}\n", m.illustrations.image_model));
                    out.push_str(&format!("  image_size:                {}\n", m.illustrations.image_size));
                    out.push_str(&format!("  image_aspect_ratio:        {}\n", m.illustrations.image_aspect_ratio));
                    out.push_str(&format!("  sentences_per_illustration: {}\n", m.illustrations.sentences_per_illustration));
                    out.push_str(&format!("  minimum_count:             {}\n", m.illustrations.minimum_count));
                    Ok(out)
                })
            }
            AppCommand::AvConfigTts { key, value } => {
                self.av_execute_mut(|producer| {
                    let tts = &mut producer.manifest.tts;
                    match key.as_str() {
                        "service" => tts.service = value.clone(),
                        "model" => tts.model = value.clone(),
                        "prompt_prefix" => tts.prompt_prefix = value.clone(),
                        "use_vertex_auth" => tts.use_vertex_auth = value.parse().map_err(|_| "Expected true or false".to_string())?,
                        "output_format" => tts.output_format = value.clone(),
                        "chunk_max_chars" => tts.chunk_max_chars = value.parse().map_err(|_| "Expected a number".to_string())?,
                        "max_api_retries" => tts.max_api_retries = value.parse().map_err(|_| "Expected a number".to_string())?,
                        "retry_delay" => tts.retry_delay = value.parse().map_err(|_| "Expected a number".to_string())?,
                        "concurrent_requests" => tts.concurrent_requests = value.parse().map_err(|_| "Expected a number".to_string())?,
                        _ => return Err(format!("Unknown TTS config key: '{}'. Valid keys: service, model, prompt_prefix, use_vertex_auth, output_format, chunk_max_chars, max_api_retries, retry_delay, concurrent_requests", key)),
                    }
                    producer.manifest.save(&producer.book_dir)?;
                    Ok(format!("TTS config '{}' set to '{}'", key, value))
                })
            }
            AppCommand::AvConfigVideo { key, value } => {
                self.av_execute_mut(|producer| {
                    let vid = &mut producer.manifest.video;
                    match key.as_str() {
                        "image_duration" => vid.image_duration = value.parse().map_err(|_| "Expected a number".to_string())?,
                        "frame_rate" => vid.frame_rate = value.parse().map_err(|_| "Expected a number".to_string())?,
                        "max_sentences_per_video" => vid.max_sentences_per_video = value.parse().map_err(|_| "Expected a number (0 = no limit)".to_string())?,
                        _ => return Err(format!("Unknown video config key: '{}'. Valid keys: image_duration, frame_rate, max_sentences_per_video", key)),
                    }
                    producer.manifest.save(&producer.book_dir)?;
                    Ok(format!("Video config '{}' set to '{}'", key, value))
                })
            }
            AppCommand::AvConfigVoices { voices } => {
                self.av_execute_mut(|producer| {
                    let count = voices.len();
                    producer.manifest.tts.voices = voices;
                    producer.manifest.save(&producer.book_dir)?;
                    Ok(format!("Set {} voice(s).", count))
                })
            }
            AppCommand::AvConfigIllustrations { key, value } => {
                self.av_execute_mut(|producer| {
                    let ill = &mut producer.manifest.illustrations;
                    match key.as_str() {
                        "style_prefix" => ill.style_prefix = value.clone(),
                        "prompt_model" => ill.prompt_model = value.clone(),
                        "image_model" => ill.image_model = value.clone(),
                        "image_size" => ill.image_size = value.clone(),
                        "image_aspect_ratio" => ill.image_aspect_ratio = value.clone(),
                        "sentences_per_illustration" => ill.sentences_per_illustration = value.parse().map_err(|_| "Expected a number".to_string())?,
                        "minimum_count" => ill.minimum_count = value.parse().map_err(|_| "Expected a number".to_string())?,
                        _ => return Err(format!("Unknown illustrations config key: '{}'. Valid keys: style_prefix, prompt_model, image_model, image_size, image_aspect_ratio, sentences_per_illustration, minimum_count", key)),
                    }
                    producer.manifest.save(&producer.book_dir)?;
                    Ok(format!("Illustrations config '{}' set to '{}'", key, value))
                })
            }
            AppCommand::AvOpenDir { which } => {
                let book_dir = self.resolve_av_book_dir()?;
                let target = match which.as_str() {
                    "book-dir" => book_dir.clone(),
                    "audio-dir" => book_dir.join("audio"),
                    "video-dir" => book_dir.join("video"),
                    "illustrations" => book_dir.join("illustrations"),
                    _ => return Err(format!("Unknown directory '{}'. Use: book-dir, audio-dir, video-dir, illustrations", which)),
                };
                if !target.exists() {
                    fs::create_dir_all(&target)
                        .map_err(|e| format!("Failed to create directory: {}", e))?;
                }
                opener::open(&target)
                    .map_err(|e| format!("Failed to open directory: {}", e))?;
                Ok(format!("Opened {}", target.display()))
            }
            AppCommand::AvGenerateAudio { target } => {
                // Reject if a job is already running
                if let Some(ref job) = self.state.av_job {
                    let j = job.lock().unwrap();
                    if !j.finished {
                        return Err(format!("AV job already running: {}. Use 'av cancel' to stop it.", j.label));
                    }
                }

                let book_dir = self.resolve_av_book_dir()?;
                let producer = crate::services::av_producer::AvProducer::new(book_dir)?;
                let project_root = self.tool_root()?;
                let api_key = crate::services::secrets::get_google_key()?;
                if api_key.trim().is_empty() {
                    return Err("Google API key is empty. Set a valid key:\n  → set key google AIza...".to_string());
                }

                let stem = match target {
                    AvTarget::Stem(ref s) => {
                        let statuses = producer.scan();
                        let found = statuses.iter().find(|st| st.stem == *s);
                        match found {
                            Some(st) if !st.has_text => return Err(format!("No text file found for '{}'.", s)),
                            Some(st) if st.has_audio => return Err(format!("Audio already exists for '{}'. Delete it first to regenerate.", s)),
                            None => return Err(format!("Stem '{}' not found in book directory.", s)),
                            _ => s.clone(),
                        }
                    }
                    AvTarget::Next => {
                        match producer.next_stem_needing_audio() {
                            Some(s) => s,
                            None => return Ok("All marked files already have audio.".to_string()),
                        }
                    }
                    AvTarget::All => {
                        // For streaming, only support one stem at a time
                        match producer.next_stem_needing_audio() {
                            Some(s) => s,
                            None => return Ok("All marked files already have audio.".to_string()),
                        }
                    }
                };

                // Spawn the child process
                let gcloud_project_id = self.state.config.as_ref()
                    .and_then(|c| c.gcloud_project_id.as_deref());
                let child = producer.spawn_audio(&stem, &project_root, &api_key, gcloud_project_id)?;
                let pid = child.id();
                let label = format!("Generating audio: {}", stem);

                let job_state = std::sync::Arc::new(std::sync::Mutex::new(
                    crate::app::state::AvJobState {
                        output_lines: vec![format!("--- {} ---", label)],
                        cancel_requested: false,
                        finished: false,
                        result_message: None,
                        child_pid: Some(pid),
                        label: label.clone(),
                    },
                ));

                // Spawn reader thread
                let job_clone = job_state.clone();
                std::thread::spawn(move || {
                    av_job_reader(child, job_clone);
                });

                self.state.av_job = Some(job_state);
                Ok(format!("Started: {} (pid {}). Use 'av cancel' to stop.", label, pid))
            }
            AppCommand::AvGenerateVideo { target } => {
                // Reject if a job is already running
                if let Some(ref job) = self.state.av_job {
                    let j = job.lock().unwrap();
                    if !j.finished {
                        return Err(format!("AV job already running: {}. Use 'av cancel' to stop it.", j.label));
                    }
                }

                let book_dir = self.resolve_av_book_dir()?;
                let producer = crate::services::av_producer::AvProducer::new(book_dir)?;
                let project_root = self.tool_root()?;

                if producer.count_illustrations() == 0 {
                    return Err(format!(
                        "No illustrations found in {}. Add images before generating video.",
                        producer.illustrations_dir().display()
                    ));
                }

                let stem = match target {
                    AvTarget::Stem(ref s) => {
                        let statuses = producer.scan();
                        let found = statuses.iter().find(|st| st.stem == *s);
                        match found {
                            Some(st) if !st.has_audio => return Err(format!("No audio file for '{}'. Generate audio first.", s)),
                            Some(st) if st.has_video => return Err(format!("Video already exists for '{}'. Delete it first to regenerate.", s)),
                            None => return Err(format!("Stem '{}' not found in book directory.", s)),
                            _ => s.clone(),
                        }
                    }
                    AvTarget::Next => {
                        match producer.next_stem_needing_video() {
                            Some(s) => s,
                            None => return Ok("All marked files with audio already have video.".to_string()),
                        }
                    }
                    AvTarget::All => {
                        match producer.next_stem_needing_video() {
                            Some(s) => s,
                            None => return Ok("All marked files with audio already have video.".to_string()),
                        }
                    }
                };

                // Check if this stem has volume audio files
                let vol_files = producer.volume_audio_files(&stem);
                let (audio_filename, video_label) = if !vol_files.is_empty() {
                    // Find first volume that doesn't have a corresponding video
                    let video_dir = producer.video_dir();
                    let mut found = None;
                    for (i, af) in vol_files.iter().enumerate() {
                        let vol_stem = format!("{}_V{}", stem, i + 1);
                        let video_file = video_dir.join(format!("{}.mp4", vol_stem));
                        if !video_file.exists() {
                            found = Some((af.clone(), vol_stem));
                            break;
                        }
                    }
                    match found {
                        Some((af, vs)) => (Some(af), vs),
                        None => return Ok(format!("All {} volume videos already exist for '{}'.", vol_files.len(), stem)),
                    }
                } else {
                    (None, stem.clone())
                };

                let child = producer.spawn_video_for_audio(
                    &video_label,
                    audio_filename.as_deref(),
                    &project_root,
                )?;
                let pid = child.id();
                let label = format!("Generating video: {}", video_label);

                let job_state = std::sync::Arc::new(std::sync::Mutex::new(
                    crate::app::state::AvJobState {
                        output_lines: vec![format!("--- {} ---", label)],
                        cancel_requested: false,
                        finished: false,
                        result_message: None,
                        child_pid: Some(pid),
                        label: label.clone(),
                    },
                ));

                let job_clone = job_state.clone();
                std::thread::spawn(move || {
                    av_job_reader(child, job_clone);
                });

                self.state.av_job = Some(job_state);
                Ok(format!("Started: {} (pid {}). Use 'av cancel' to stop.", label, pid))
            }
            AppCommand::AvGenerateCharacters => {
                // Reject if a job is already running
                if let Some(ref job) = self.state.av_job {
                    let j = job.lock().unwrap();
                    if !j.finished {
                        return Err(format!("AV job already running: {}. Use 'av cancel' to stop it.", j.label));
                    }
                }

                let book_dir = self.resolve_av_book_dir()?;
                let producer = crate::services::av_producer::AvProducer::new(book_dir)?;
                let project_root = self.tool_root()?;
                let api_key = crate::services::secrets::get_google_key()?;
                if api_key.trim().is_empty() {
                    return Err("Google API key is empty. Set a valid key:\n  → set key google AIza...".to_string());
                }

                let book_name = self.state.book_name.clone();

                let child = producer.spawn_extract_characters(&book_name, &project_root, &api_key)?;
                let pid = child.id();
                let label = "Extracting character bible".to_string();

                let job_state = std::sync::Arc::new(std::sync::Mutex::new(
                    crate::app::state::AvJobState {
                        output_lines: vec![format!("--- {} ---", label)],
                        cancel_requested: false,
                        finished: false,
                        result_message: None,
                        child_pid: Some(pid),
                        label: label.clone(),
                    },
                ));

                let job_clone = job_state.clone();
                std::thread::spawn(move || {
                    av_job_reader(child, job_clone);
                });

                self.state.av_job = Some(job_state);
                Ok(format!("Started: {} (pid {}). Use 'av cancel' to stop.", label, pid))
            }
            AppCommand::AvGeneratePrompts => {
                // Reject if a job is already running
                if let Some(ref job) = self.state.av_job {
                    let j = job.lock().unwrap();
                    if !j.finished {
                        return Err(format!("AV job already running: {}. Use 'av cancel' to stop it.", j.label));
                    }
                }

                let book_dir = self.resolve_av_book_dir()?;
                let producer = crate::services::av_producer::AvProducer::new(book_dir)?;
                let project_root = self.tool_root()?;
                let api_key = crate::services::secrets::get_google_key()?;
                if api_key.trim().is_empty() {
                    return Err("Google API key is empty. Set a valid key:\n  → set key google AIza...".to_string());
                }

                let book_name = self.state.book_name.clone();

                let child = producer.spawn_prompts(&book_name, &project_root, &api_key)?;
                let pid = child.id();
                let label = "Generating illustration prompts".to_string();

                let job_state = std::sync::Arc::new(std::sync::Mutex::new(
                    crate::app::state::AvJobState {
                        output_lines: vec![format!("--- {} ---", label)],
                        cancel_requested: false,
                        finished: false,
                        result_message: None,
                        child_pid: Some(pid),
                        label: label.clone(),
                    },
                ));

                let job_clone = job_state.clone();
                std::thread::spawn(move || {
                    av_job_reader(child, job_clone);
                });

                self.state.av_job = Some(job_state);
                Ok(format!("Started: {} (pid {}). Use 'av cancel' to stop.", label, pid))
            }
            AppCommand::AvGenerateIllustrations => {
                // Reject if a job is already running
                if let Some(ref job) = self.state.av_job {
                    let j = job.lock().unwrap();
                    if !j.finished {
                        return Err(format!("AV job already running: {}. Use 'av cancel' to stop it.", j.label));
                    }
                }

                let book_dir = self.resolve_av_book_dir()?;
                let producer = crate::services::av_producer::AvProducer::new(book_dir)?;
                let project_root = self.tool_root()?;
                let api_key = crate::services::secrets::get_google_key()?;
                if api_key.trim().is_empty() {
                    return Err("Google API key is empty. Set a valid key:\n  → set key google AIza...".to_string());
                }

                if !producer.has_prompts() {
                    return Err("No _prompts.toml found. Run 'av generate prompts' first.".to_string());
                }

                let child = producer.spawn_illustrations(&project_root, &api_key)?;
                let pid = child.id();
                let label = "Generating illustrations".to_string();

                let job_state = std::sync::Arc::new(std::sync::Mutex::new(
                    crate::app::state::AvJobState {
                        output_lines: vec![format!("--- {} ---", label)],
                        cancel_requested: false,
                        finished: false,
                        result_message: None,
                        child_pid: Some(pid),
                        label: label.clone(),
                    },
                ));

                let job_clone = job_state.clone();
                std::thread::spawn(move || {
                    av_job_reader(child, job_clone);
                });

                self.state.av_job = Some(job_state);
                Ok(format!("Started: {} (pid {}). Use 'av cancel' to stop.", label, pid))
            }
            AppCommand::AvCancel => {
                if let Some(ref job) = self.state.av_job {
                    let mut j = job.lock().unwrap();
                    if j.finished {
                        return Ok("No AV job is running.".to_string());
                    }
                    j.cancel_requested = true;
                    // Kill the child process
                    if let Some(pid) = j.child_pid {
                        // On Windows, kill the process tree
                        let _ = std::process::Command::new("taskkill")
                            .args(["/PID", &pid.to_string(), "/T", "/F"])
                            .output();
                    }
                    Ok("Cancel requested. The AV job will stop shortly.".to_string())
                } else {
                    Ok("No AV job is running.".to_string())
                }
            }

            AppCommand::AvLog { tail } => {
                if let Some(ref job) = self.state.av_job {
                    let j = job.lock().unwrap();
                    let lines = &j.output_lines;
                    let selected: Vec<&str> = match tail {
                        Some(n) if n < lines.len() => lines[lines.len() - n..].iter().map(|s| s.as_str()).collect(),
                        _ => lines.iter().map(|s| s.as_str()).collect(),
                    };
                    if selected.is_empty() {
                        Ok("(no output yet)".to_string())
                    } else {
                        Ok(selected.join("\n"))
                    }
                } else {
                    Ok("No AV job (current or recent).".to_string())
                }
            }

            AppCommand::AvChunkStatus { stem } => {
                self.av_execute(|producer| {
                    let chunks = producer.scan_chunks(&stem);
                    if chunks.is_empty() {
                        return Ok(format!("No chunks found for '{}'. Generate audio first.", stem));
                    }
                    let mut out = format!("Chunks for '{}':\n", stem);
                    out.push_str(&format!("{:>5}  {:>5}  {:>5}  {}\n", "Index", "Text", "Audio", "Status"));
                    out.push_str(&format!("{}\n", "-".repeat(35)));
                    for c in &chunks {
                        let status = if c.is_rejected {
                            "REJECTED"
                        } else if c.has_audio {
                            "ok"
                        } else {
                            "MISSING"
                        };
                        let txt = if c.has_text { "✓" } else { "-" };
                        let aud = if c.has_audio { "✓" } else if c.is_rejected { "✗" } else { "-" };
                        out.push_str(&format!("{:>5}  {:>5}  {:>5}  {}\n", c.index, txt, aud, status));
                    }
                    let good = chunks.iter().filter(|c| c.has_audio && !c.is_rejected).count();
                    let rejected = chunks.iter().filter(|c| c.is_rejected).count();
                    let missing = chunks.iter().filter(|c| !c.has_audio && !c.is_rejected && c.has_text).count();
                    out.push_str(&format!("\nTotal: {} | Good: {} | Rejected: {} | Missing: {}\n",
                        chunks.len(), good, rejected, missing));
                    Ok(out)
                })
            }

            AppCommand::AvRejectChunk { stem, index } => {
                self.av_execute(|producer| {
                    let chunks_dir = producer.chunks_dir(&stem);
                    if !chunks_dir.exists() {
                        return Err(format!("No chunks directory for '{}'.", stem));
                    }
                    let prefix = format!("temp_chunk_{:04}", index);
                    let wav = chunks_dir.join(format!("{}.wav", prefix));
                    let silence = chunks_dir.join(format!("{}_silence.wav", prefix));
                    if wav.exists() {
                        let bad = chunks_dir.join(format!("{}.wav.bad", prefix));
                        std::fs::rename(&wav, &bad)
                            .map_err(|e| format!("Failed to rename: {}", e))?;
                        Ok(format!("Rejected chunk {} for '{}'.", index, stem))
                    } else if silence.exists() {
                        let bad = chunks_dir.join(format!("{}_silence.wav.bad", prefix));
                        std::fs::rename(&silence, &bad)
                            .map_err(|e| format!("Failed to rename: {}", e))?;
                        Ok(format!("Rejected silence chunk {} for '{}'.", index, stem))
                    } else {
                        Err(format!("No audio file found for chunk {} of '{}'.", index, stem))
                    }
                })
            }

            AppCommand::AvRestoreChunk { stem, index } => {
                self.av_execute(|producer| {
                    let chunks_dir = producer.chunks_dir(&stem);
                    if !chunks_dir.exists() {
                        return Err(format!("No chunks directory for '{}'.", stem));
                    }
                    let prefix = format!("temp_chunk_{:04}", index);
                    let bad_wav = chunks_dir.join(format!("{}.wav.bad", prefix));
                    let bad_silence = chunks_dir.join(format!("{}_silence.wav.bad", prefix));
                    if bad_wav.exists() {
                        let wav = chunks_dir.join(format!("{}.wav", prefix));
                        std::fs::rename(&bad_wav, &wav)
                            .map_err(|e| format!("Failed to rename: {}", e))?;
                        Ok(format!("Restored chunk {} for '{}'.", index, stem))
                    } else if bad_silence.exists() {
                        let silence = chunks_dir.join(format!("{}_silence.wav", prefix));
                        std::fs::rename(&bad_silence, &silence)
                            .map_err(|e| format!("Failed to rename: {}", e))?;
                        Ok(format!("Restored silence chunk {} for '{}'.", index, stem))
                    } else {
                        Err(format!("No rejected (.wav.bad) file found for chunk {} of '{}'.", index, stem))
                    }
                })
            }

            AppCommand::AvRebuildAudio { stem } => {
                let book_dir = self.resolve_av_book_dir()?;
                let producer = crate::services::av_producer::AvProducer::new(book_dir)?;
                let project_root = self.tool_root()?;
                producer.rebuild_audio(&stem, &project_root, self.state.document.len() as u32)
            }

            // --- YouTube Commands ---
            AppCommand::AvYoutubeInit => {
                let book_dir = self.resolve_av_book_dir()?;
                let yt = crate::services::av_producer::YouTubeConfig::load_or_create(&book_dir)?;
                let _ = yt; // just ensure it's created
                Ok(format!("YouTube config created at: {}", book_dir.join(crate::services::av_producer::YOUTUBE_CONFIG_FILENAME).display()))
            }
            AppCommand::AvYoutubeAuth => {
                // Reject if a job is already running
                if let Some(ref job) = self.state.av_job {
                    let j = job.lock().unwrap();
                    if !j.finished {
                        return Err(format!("AV job already running: {}. Use 'av cancel' to stop it.", j.label));
                    }
                }

                let book_dir = self.resolve_av_book_dir()?;
                let _producer = crate::services::av_producer::AvProducer::new(book_dir)?;
                let project_root = self.tool_root()?;

                let child = _producer.spawn_youtube_auth(
                    &project_root,
                    self.state.config.as_ref()
                        .and_then(|c| c.youtube_client_secret_file.as_deref()),
                )?;
                let pid = child.id();
                let label = "YouTube OAuth authentication".to_string();

                let job_state = std::sync::Arc::new(std::sync::Mutex::new(
                    crate::app::state::AvJobState {
                        output_lines: vec![format!("--- {} ---", label)],
                        cancel_requested: false,
                        finished: false,
                        result_message: None,
                        child_pid: Some(pid),
                        label: label.clone(),
                    },
                ));

                let job_clone = job_state.clone();
                std::thread::spawn(move || {
                    av_job_reader(child, job_clone);
                });

                self.state.av_job = Some(job_state);
                Ok(format!("Started: {} (pid {}). A browser window should open for consent.", label, pid))
            }
            AppCommand::AvYoutubeConfigShow => {
                let book_dir = self.resolve_av_book_dir()?;
                let yt = crate::services::av_producer::YouTubeConfig::load(&book_dir)?
                    .ok_or("No _youtube.toml found. Run 'av youtube init' first.")?;
                let mut out = String::new();
                out.push_str("--- YouTube Config ---\n");
                out.push_str(&format!("  title_template:       {}\n", yt.metadata.title_template));
                out.push_str(&format!("  description_template: {}\n", yt.metadata.description_template.lines().next().unwrap_or("")));
                out.push_str(&format!("  tags:                 {}\n", yt.metadata.tags.join(", ")));
                out.push_str(&format!("  category_id:          {}\n", yt.metadata.category_id));
                out.push_str(&format!("  privacy:              {}\n", yt.metadata.privacy));
                out.push_str(&format!("  language:             {}\n", yt.metadata.language));
                out.push_str(&format!("  client_secret_file:   {}\n", if yt.auth.client_secret_file.is_empty() { "(not set)" } else { &yt.auth.client_secret_file }));
                if !yt.variables.is_empty() {
                    out.push_str("  variables:\n");
                    for (k, v) in &yt.variables {
                        out.push_str(&format!("    {} = {}\n", k, v));
                    }
                }
                if !yt.uploads.is_empty() {
                    out.push_str(&format!("  uploads:              {} video(s) uploaded\n", yt.uploads.len()));
                }
                Ok(out)
            }
            AppCommand::AvYoutubeConfig { key, value } => {
                let book_dir = self.resolve_av_book_dir()?;
                let mut yt = crate::services::av_producer::YouTubeConfig::load(&book_dir)?
                    .ok_or("No _youtube.toml found. Run 'av youtube init' first.")?;
                match key.as_str() {
                    "title_template" => yt.metadata.title_template = value.clone(),
                    "description_template" => yt.metadata.description_template = value.clone(),
                    "tags" => yt.metadata.tags = value.split(',').map(|s| s.trim().to_string()).collect(),
                    "category_id" => yt.metadata.category_id = value.clone(),
                    "privacy" => {
                        if !["public", "unlisted", "private"].contains(&value.as_str()) {
                            return Err("Privacy must be: public, unlisted, or private".to_string());
                        }
                        yt.metadata.privacy = value.clone();
                    }
                    "language" => yt.metadata.language = value.clone(),
                    "client_secret_file" => yt.auth.client_secret_file = value.clone(),
                    _ => {
                        // Treat unknown keys as template variables
                        yt.variables.insert(key.clone(), value.clone());
                    }
                }
                yt.save(&book_dir)?;
                Ok(format!("YouTube config '{}' set to '{}'", key, value))
            }
            AppCommand::AvYoutubeUpload { target } => {
                // Reject if a job is already running
                if let Some(ref job) = self.state.av_job {
                    let j = job.lock().unwrap();
                    if !j.finished {
                        return Err(format!("AV job already running: {}. Use 'av cancel' to stop it.", j.label));
                    }
                }

                let book_dir = self.resolve_av_book_dir()?;
                let producer = crate::services::av_producer::AvProducer::new(book_dir.clone())?;
                let project_root = self.tool_root()?;
                let yt = crate::services::av_producer::YouTubeConfig::load(&book_dir)?
                    .ok_or("No _youtube.toml found. Run 'av youtube init' first.")?;

                let stem = match target {
                    AvTarget::Stem(ref s) => {
                        if yt.is_uploaded(s) {
                            return Err(format!("'{}' already uploaded. Delete the [uploads] entry in _youtube.toml to re-upload.", s));
                        }
                        let statuses = producer.scan();
                        let found = statuses.iter().find(|st| st.stem == *s);
                        match found {
                            Some(st) if !st.has_video => return Err(format!("No video file for '{}'. Generate video first.", s)),
                            None => return Err(format!("Stem '{}' not found in book directory.", s)),
                            _ => s.clone(),
                        }
                    }
                    AvTarget::Next => {
                        match producer.next_stem_needing_upload(&yt) {
                            Some(s) => s,
                            None => return Ok("All marked videos have been uploaded.".to_string()),
                        }
                    }
                    AvTarget::All => {
                        match producer.next_stem_needing_upload(&yt) {
                            Some(s) => s,
                            None => return Ok("All marked videos have been uploaded.".to_string()),
                        }
                    }
                };

                // Build extra template variables from state
                let book_name = self.state.book_name.clone();
                let chapter_name = book_dir.file_name()
                    .map(|n| n.to_string_lossy().replace('_', " "))
                    .unwrap_or_default();
                let extra_vars = format!(
                    "book_name={},chapter_name={}",
                    book_name, chapter_name
                );

                let child = producer.spawn_youtube_upload(
                    &stem,
                    &project_root,
                    &extra_vars,
                    self.state.config.as_ref()
                        .and_then(|c| c.youtube_client_secret_file.as_deref()),
                )?;
                let pid = child.id();
                let label = format!("Uploading to YouTube: {}", stem);

                let job_state = std::sync::Arc::new(std::sync::Mutex::new(
                    crate::app::state::AvJobState {
                        output_lines: vec![format!("--- {} ---", label)],
                        cancel_requested: false,
                        finished: false,
                        result_message: None,
                        child_pid: Some(pid),
                        label: label.clone(),
                    },
                ));

                let job_clone = job_state.clone();
                std::thread::spawn(move || {
                    av_job_reader(child, job_clone);
                });

                self.state.av_job = Some(job_state);
                Ok(format!("Started: {} (pid {}). Use 'av cancel' to stop.", label, pid))
            }

            // --- Chapter Mode Commands ---
            AppCommand::NewChapter { name, start, end } => {
                self.execute_new_chapter(name, start, end)
            }
            AppCommand::ListChapters => {
                self.execute_list_chapters()
            }
            AppCommand::DeleteChapter { name } => {
                self.execute_delete_chapter(&name)
            }
            AppCommand::SelectChapter { name } => {
                self.execute_select_chapter(&name)
            }
            AppCommand::SetChapterMode { enabled } => {
                self.state.chapter_mode = enabled;
                let mode_str = if enabled { "Chapter" } else { "Book" };
                Ok(format!("Mode set to {}.", mode_str))
            }
            AppCommand::SetFrontierEnabled { enabled } => {
                self.state.frontier_enabled = enabled;
                Ok(format!("Frontier {}.", if enabled { "enabled" } else { "disabled" }))
            }
            AppCommand::SetFrontierPct { pct } => {
                self.state.frontier_target_pct = pct;
                Ok(format!("Frontier target set to {}%.", pct))
            }
            AppCommand::SetFrontierSeed { seed } => {
                self.state.frontier_seed = seed;
                Ok(format!("Frontier seed set to {}.", seed))
            }
            AppCommand::InitMediaWorkspace => {
                self.execute_init_media_workspace()
            }

            AppCommand::CopilotJournal { text } => {
                let ws_dir = self.state.config.as_ref()
                    .map(|c| c.content_project_dir.clone())
                    .unwrap_or_default();
                if ws_dir.is_empty() {
                    return Err("No workspace open.".to_string());
                }
                crate::services::copilot::append_journal(
                    std::path::Path::new(&ws_dir),
                    &text,
                );
                Ok(format!("Journal entry added."))
            }

            AppCommand::CopilotReset => {
                self.state.copilot_history.clear();
                self.state.copilot_turns = 0;
                self.state.copilot_auto_turns = 0;
                self.state.copilot_running = false;
                self.state.copilot_awaiting_av = false;
                self.state.copilot_awaiting_llm_job = false;
                self.state.copilot_pending_cmds.clear();
                self.state.copilot_cmd_outputs.clear();
                self.state.copilot_llm_rx = None;
                // Delete session file
                let ws_dir = self.state.config.as_ref()
                    .map(|c| c.content_project_dir.clone())
                    .unwrap_or_default();
                if !ws_dir.is_empty() {
                    let session_path = std::path::Path::new(&ws_dir)
                        .join("copilot")
                        .join("_session.json");
                    let _ = std::fs::remove_file(&session_path);
                }
                Ok("Copilot session reset. History cleared.".to_string())
            }
        }
    }

    // ----- AV Production helpers -----

    /// Resolve the book output directory from output_dir + book_name.
    /// In chapter mode, returns the selected chapter's subdirectory.
    fn resolve_av_book_dir(&self) -> Result<PathBuf, String> {
        let output_dir = self.state.output_dir.as_ref()
            .ok_or("Output directory not set. Use 'set output_dir <path>' first.")?;
        let book_dir = crate::services::av_producer::AvProducer::resolve_book_dir(output_dir, &self.state.book_name);

        let target_dir = if self.state.chapter_mode {
            let ch_idx = self.state.selected_chapter_idx
                .ok_or("Chapter mode is on but no chapter selected.")?;
            let ch = self.state.chapters.get(ch_idx)
                .ok_or("Selected chapter index is invalid.")?;
            let ch_dir_name = Self::sanitize_name(&ch.name);
            book_dir.join(ch_dir_name)
        } else {
            book_dir.join("whole_book")
        };

        if !target_dir.exists() {
            return Err(format!("Directory does not exist: {}. Run 'init media' first.", target_dir.display()));
        }
        Ok(target_dir)
    }

    /// Execute an AV operation with an immutable AvProducer.
    fn av_execute<F>(&self, f: F) -> Result<String, String>
    where
        F: FnOnce(&crate::services::av_producer::AvProducer) -> Result<String, String>,
    {
        let book_dir = self.resolve_av_book_dir()?;
        let producer = crate::services::av_producer::AvProducer::new(book_dir)?;
        f(&producer)
    }

    /// Execute an AV operation with a mutable AvProducer.
    fn av_execute_mut<F>(&self, f: F) -> Result<String, String>
    where
        F: FnOnce(&mut crate::services::av_producer::AvProducer) -> Result<String, String>,
    {
        let book_dir = self.resolve_av_book_dir()?;
        let mut producer = crate::services::av_producer::AvProducer::new(book_dir)?;
        f(&mut producer)
    }

    fn execute_measure_avd(&self, path: &str) -> Result<String, String> {
        use crate::simulation::frequency_manager;
        use crate::simulation::metrics::TextMetrics;

        // Verify frequency list is loaded
        if frequency_manager::get_max_rank() == 0 {
            return Err("Frequency list not loaded. Cannot compute AVD.".to_string());
        }

        // Read the file
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file '{}': {}", path, e))?;

        if content.trim().is_empty() {
            return Err("File is empty.".to_string());
        }

        // Tokenize via SpaCy bridge to get lemmas
        let bridge = self.state.bridge.as_ref()
            .ok_or("Python Bridge not available. Cannot tokenize text.")?;

        let raw_tokens = bridge.tokenize(content.trim(), "es")
            .map_err(|e| format!("SpaCy tokenization failed: {}", e))?;

        // Extract lemma instances (skip punctuation and whitespace)
        let mut lemma_instances: Vec<String> = Vec::new();
        let mut unknown_lemmas: Vec<String> = Vec::new();
        let mut total_word_tokens = 0u32;

        for token in &raw_tokens {
            if token.is_punct || token.is_space {
                continue;
            }
            total_word_tokens += 1;
            let lemma = token.lemma.to_lowercase();
            if frequency_manager::get_rank_for_lemma(&lemma).is_none() {
                if !unknown_lemmas.contains(&lemma) {
                    unknown_lemmas.push(lemma.clone());
                }
            }
            lemma_instances.push(lemma);
        }

        if lemma_instances.is_empty() {
            return Err("No word tokens found in text.".to_string());
        }

        // Compute AVD using TextMetrics (english_word_count = 0 for pure Spanish text)
        let metrics = TextMetrics::new(&lemma_instances, 0);
        let avd_score = metrics.calculate_avd_score();

        // Find the highest-ranked lemma for context
        let mut max_rank: u32 = 0;
        let mut max_rank_lemma = String::new();
        for lemma in &lemma_instances {
            if let Some(rank) = frequency_manager::get_rank_for_lemma(lemma) {
                if rank > max_rank {
                    max_rank = rank;
                    max_rank_lemma = lemma.clone();
                }
            }
        }

        // Build output report
        let found_count = lemma_instances.iter()
            .filter(|l| frequency_manager::get_rank_for_lemma(l).is_some())
            .count();

        let mut out = String::new();
        out.push_str(&format!("--- AVD Measurement for '{}' ---\n", path));
        out.push_str(&format!("  Total word tokens:    {}\n", total_word_tokens));
        out.push_str(&format!("  In frequency list:    {}\n", found_count));
        out.push_str(&format!("  Unknown lemmas:       {}\n", unknown_lemmas.len()));
        out.push_str(&format!("  AVD Score:            {:.2}\n", avd_score));
        out.push_str(&format!("  Highest ranked lemma: '{}' (rank {})\n", max_rank_lemma, max_rank));

        if !unknown_lemmas.is_empty() {
            unknown_lemmas.sort();
            let display: Vec<&str> = unknown_lemmas.iter().map(|s| s.as_str()).take(20).collect();
            out.push_str(&format!("  Unknown sample:       {}", display.join(", ")));
            if unknown_lemmas.len() > 20 {
                out.push_str(&format!(" ... (+{} more)", unknown_lemmas.len() - 20));
            }
        }

        Ok(out)
    }

    fn execute_measure_user_score(&self, path: &str) -> Result<String, String> {
        use crate::simulation::frequency_manager;
        use crate::simulation::metrics::TextMetrics;

        use crate::simulation::calibrator;

        // Verify frequency list is loaded
        if frequency_manager::get_max_rank() == 0 {
            return Err("Frequency list not loaded. Cannot compute AVD.".to_string());
        }

        // Read and tokenize
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file '{}': {}", path, e))?;

        if content.trim().is_empty() {
            return Err("File is empty.".to_string());
        }

        let bridge = self.state.bridge.as_ref()
            .ok_or("Python Bridge not available. Cannot tokenize text.")?;

        let raw_tokens = bridge.tokenize(content.trim(), "es")
            .map_err(|e| format!("SpaCy tokenization failed: {}", e))?;

        let mut lemma_instances: Vec<String> = Vec::new();
        let mut total_word_tokens = 0u32;

        for token in &raw_tokens {
            if token.is_punct || token.is_space {
                continue;
            }
            total_word_tokens += 1;
            lemma_instances.push(token.lemma.to_lowercase());
        }

        if lemma_instances.is_empty() {
            return Err("No word tokens found in text.".to_string());
        }

        // Compute AVD
        let metrics = TextMetrics::new(&lemma_instances, 0);
        let avd_score = metrics.calculate_avd_score();

        // Inverse mapping: AVD -> User Level
        let user_level = calibrator::get_user_level_from_avd(avd_score);

        // Find unique lemma count and coverage stats
        let mut unique_lemmas: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for lemma in &lemma_instances {
            unique_lemmas.insert(lemma.as_str());
        }

        let in_freq_list = lemma_instances.iter()
            .filter(|l| frequency_manager::get_rank_for_lemma(l).is_some())
            .count();

        let mut out = String::new();
        out.push_str(&format!("--- User Score Measurement for '{}' ---\n", path));
        out.push_str(&format!("  Total word tokens:    {}\n", total_word_tokens));
        out.push_str(&format!("  Unique lemmas:        {}\n", unique_lemmas.len()));
        out.push_str(&format!("  In frequency list:    {} / {}\n", in_freq_list, lemma_instances.len()));
        out.push_str(&format!("  AVD Score:            {:.2}\n", avd_score));
        out.push_str(&format!("  Estimated User Level: {:.1}\n", user_level));
        out.push_str(&format!("  (Rounded):            UL{}", user_level.round() as u32));

        Ok(out)
    }

    fn execute_export_json(&self, path: &str) -> Result<String, String> {
        let path_buf = self.resolve_path(path);

        use crate::domain::bridge::domain_sentences_to_json_chapter;
        let (base_lang, target_lang) = &self.state.project_languages;
        let json_chapter = domain_sentences_to_json_chapter(
            &self.state.document,
            &self.state.book_name,
            base_lang,
            target_lang,
            self.state.book_map.as_ref(),
        );

        let json_str = serde_json::to_string_pretty(&json_chapter)
            .map_err(|e| format!("Failed to serialize JSON: {}", e))?;
            
        if let Some(parent) = path_buf.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        std::fs::write(&path_buf, json_str)
            .map_err(|e| format!("Failed to write to {}: {}", path_buf.display(), e))?;

        Ok(format!("Exported JSON to {}", path_buf.display()))
    }

    fn execute_export_level_map(&self, path: &str) -> Result<String, String> {
        // Validate that we have a level map
        let book_map = self.state.book_map.as_ref()
            .ok_or("No level map available. Load a calibrated project or JSON file first.")?;

        if book_map.is_empty() {
            return Err("Level map is empty. The calibrator has not been run on this project.".to_string());
        }

        // AVD formula constants (from calibrator.rs)
        const A_FIT: f64 = 4.15;
        const B_FIT: f64 = 0.02;

        // Detect the natural peak: scan ALL map entries across ALL start_levels
        // to find the last micro-level where at least one recipe tier is NOT
        // u32::MAX.  Each start_level key's map only covers its own range,
        // so we must look at all of them.
        let natural_peak: u32;
        let mut peak_micro_level: f64 = 1.0;

        for (_key, curriculum_map) in book_map.iter() {
            for entry in &curriculum_map.map {
                let all_maxed = entry.recipe.bas == u32::MAX
                    && entry.recipe.mod_v == u32::MAX
                    && entry.recipe.adv == u32::MAX;
                if !all_maxed {
                    let lvl = entry.level as f64;
                    if lvl > peak_micro_level {
                        peak_micro_level = lvl;
                    }
                }
            }
        }
        natural_peak = peak_micro_level.floor() as u32;
        let peak_avd_from_map = ((peak_micro_level - B_FIT) / A_FIT).exp() - 1.0;

        // The fractional peak user score is the exact last non-exhausted micro-level
        let peak_user_score: f64 = peak_micro_level;

        let score_int = peak_user_score.floor() as u32;
        let score_frac = ((peak_user_score - score_int as f64) * 10.0).round() as u32;

        // Determine book name
        let book_name = if self.state.book_name.is_empty() {
            "Unknown"
        } else {
            &self.state.book_name
        };

        // Build the total start_levels count (only include non-exhausted ones)
        let total_start_levels = book_map.keys()
            .filter_map(|k| k.parse::<u32>().ok())
            .filter(|&k| k <= natural_peak)
            .count() as u32;

        // Build the LevelMapFile with metadata
        use crate::types::json_types::{LevelMapFile, LevelMapMeta};

        // Filter out levels past the natural peak
        let trimmed_levels: HashMap<String, crate::types::json_types::JsonCurriculumMap> = book_map
            .iter()
            .filter(|(k, _)| {
                k.parse::<u32>().map_or(false, |kv| kv <= natural_peak)
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let lm_file = LevelMapFile {
            meta: LevelMapMeta {
                book_name: book_name.to_string(),
                base_language: self.state.project_languages.0.clone(),
                target_language: self.state.project_languages.1.clone(),
                natural_peak_level: natural_peak,
                peak_avd: (peak_avd_from_map * 100.0).round() / 100.0,
                peak_user_score: (peak_user_score * 10.0).round() / 10.0,
                total_start_levels,
                schema_version: "1.0".to_string(),
                calibration_sentence_count: self.state.calibration_sentence_count,
            },
            levels: trimmed_levels,
        };

        // Resolve output path
        let path_buf = self.resolve_path(path);
        let output_path = if path_buf.extension().map_or(false, |ext| ext == "lm") {
            // User provided a full filename
            path_buf
        } else {
            // User provided a directory — generate default name
            let default_name = format!("{}_UL{}p{}.lm", book_name, score_int, score_frac);
            path_buf.join(default_name)
        };

        // Serialize the level map file
        let json = serde_json::to_string_pretty(&lm_file)
            .map_err(|e| format!("Failed to serialize level map: {}", e))?;

        // Write the file
        fs::write(&output_path, json)
            .map_err(|e| format!("Failed to write '{}': {}", output_path.display(), e))?;

        let entry_count: usize = lm_file.levels.values().map(|m| m.map.len()).sum();
        Ok(format!(
            "Exported level map to '{}'\n  Book: {}\n  Natural peak: UL{}\n  Peak AVD: {:.2}\n  Peak user score: {:.1}\n  Start levels: {}\n  Total map entries: {}",
            output_path.display(),
            book_name,
            natural_peak,
            peak_avd_from_map,
            peak_user_score,
            total_start_levels,
            entry_count
        ))
    }

    /// Show the level map — either open a full HTML view in the browser,
    /// or dump a single level's entries to the terminal.
    fn execute_show_level_map(&self, level: Option<u32>) -> Result<String, String> {
        let book_map = self.state.book_map.as_ref()
            .ok_or("No level map loaded. Use 'calibrate' or 'import level_map' first.")?;
        if book_map.is_empty() {
            return Err("Level map is empty.".to_string());
        }

        // Collect and sort levels numerically
        let mut sorted_keys: Vec<u32> = book_map.keys()
            .filter_map(|k| k.parse::<u32>().ok())
            .collect();
        sorted_keys.sort();

        let total_sentences = self.state.document.len();
        let book_name = if self.state.book_name.is_empty() { "Untitled" } else { &self.state.book_name };

        // Single-level terminal dump
        if let Some(lvl) = level {
            let key = lvl.to_string();
            let cm = book_map.get(&key)
                .ok_or(format!("No level map entry for UL{}. Available: {}", lvl,
                    sorted_keys.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(", ")))?;

            let mut out = format!("=== UL{} (end_level: {:.1}, {} entries) ===\n", lvl, cm.end_level, cm.map.len());
            out.push_str(&format!("{:<8} {:<14} {:<18} {:<18}\n",
                "Level", "Sentences", "V-Recipe (B/M/A)", "L-Recipe (B/M/A)"));
            out.push_str(&format!("{}\n", "-".repeat(60)));

            for (i, entry) in cm.map.iter().enumerate() {
                let start = entry.start_sentence_idx;
                let end = if i + 1 < cm.map.len() {
                    cm.map[i + 1].start_sentence_idx.saturating_sub(1)
                } else {
                    total_sentences.saturating_sub(1)
                };
                let count = end.saturating_sub(start) + 1;

                let v_recipe = if entry.recipe.bas == u32::MAX {
                    "MAX/MAX/MAX".to_string()
                } else {
                    format!("{}/{}/{}", entry.recipe.bas, entry.recipe.mod_v, entry.recipe.adv)
                };
                let l_recipe = format!("{:.2}/{:.2}/{:.2}",
                    entry.l_level_recipe.bas, entry.l_level_recipe.mod_v, entry.l_level_recipe.adv);

                out.push_str(&format!("{:<8.1} S{}-S{} ({:>4})  {:<18} {:<18}\n",
                    entry.level, start + 1, end + 1, count, v_recipe, l_recipe));
            }
            return Ok(out);
        }

        // Full HTML view — generate and open in browser
        let mut html = String::with_capacity(8192);
        html.push_str("<!DOCTYPE html><html><head><meta charset='utf-8'>\n");
        html.push_str("<title>Level Map — ");
        html.push_str(&html_escape(book_name));
        html.push_str("</title>\n<style>\n");
        html.push_str("body { font-family: 'Segoe UI', system-ui, sans-serif; max-width: 1000px; margin: 20px auto; padding: 0 20px; background: #1e1e2e; color: #cdd6f4; }\n");
        html.push_str("h1 { color: #89b4fa; border-bottom: 2px solid #45475a; padding-bottom: 8px; }\n");
        html.push_str(".meta { background: #313244; padding: 12px 16px; border-radius: 6px; margin-bottom: 16px; }\n");
        html.push_str("details { margin: 4px 0; }\n");
        html.push_str("summary { cursor: pointer; padding: 8px 12px; background: #313244; border-radius: 4px; font-weight: 600; }\n");
        html.push_str("summary:hover { background: #45475a; }\n");
        html.push_str("table { width: 100%; border-collapse: collapse; margin: 8px 0 12px 0; font-size: 0.9em; }\n");
        html.push_str("th { text-align: left; padding: 6px 10px; background: #45475a; color: #cdd6f4; }\n");
        html.push_str("td { padding: 4px 10px; border-bottom: 1px solid #313244; }\n");
        html.push_str("tr:hover td { background: #313244; }\n");
        html.push_str(".bar-cell { position: relative; width: 120px; }\n");
        html.push_str(".bar { height: 14px; display: inline-block; }\n");
        html.push_str(".bar-bas { background: #89b4fa; }\n");
        html.push_str(".bar-mod { background: #a6e3a1; }\n");
        html.push_str(".bar-adv { background: #f9e2af; }\n");
        html.push_str(".legend { font-size: 0.85em; color: #a6adc8; margin: 4px 0 8px 12px; }\n");
        html.push_str(".legend span { margin-right: 12px; }\n");
        html.push_str("</style></head><body>\n");

        html.push_str(&format!("<h1>Level Map — {}</h1>\n", html_escape(book_name)));
        html.push_str(&format!("<div class='meta'>Total sentences: {} &nbsp;|&nbsp; Start levels: {} &nbsp;|&nbsp; Range: UL{}–UL{}</div>\n",
            total_sentences, sorted_keys.len(),
            sorted_keys.first().unwrap_or(&0), sorted_keys.last().unwrap_or(&0)));
        html.push_str("<div class='legend'><span>&#9632; <span style='color:#89b4fa'>Basic</span></span><span>&#9632; <span style='color:#a6e3a1'>Moderate</span></span><span>&#9632; <span style='color:#f9e2af'>Advanced</span></span></div>\n");

        for &lvl in &sorted_keys {
            let key = lvl.to_string();
            let cm = match book_map.get(&key) { Some(c) => c, None => continue };

            // Calculate summary for the level header
            let first = cm.map.first();
            let last = cm.map.last();
            let summary_recipe = first.map(|e| {
                if e.recipe.bas == u32::MAX { "MAX".to_string() }
                else { format!("{}/{}/{}", e.recipe.bas, e.recipe.mod_v, e.recipe.adv) }
            }).unwrap_or_default();
            let end_recipe = last.map(|e| {
                if e.recipe.bas == u32::MAX { "MAX".to_string() }
                else { format!("{}/{}/{}", e.recipe.bas, e.recipe.mod_v, e.recipe.adv) }
            }).unwrap_or_default();

            html.push_str(&format!(
                "<details><summary>UL{} &nbsp;→&nbsp; UL{:.1} &emsp; ({} entries) &emsp; Recipe: {} → {}</summary>\n",
                lvl, cm.end_level, cm.map.len(), summary_recipe, end_recipe));

            html.push_str("<table><tr><th>Level</th><th>Sentences</th><th>Count</th><th>V-Recipe (B/M/A)</th><th>L-Recipe (B/M/A)</th><th>Mix</th></tr>\n");

            for (i, entry) in cm.map.iter().enumerate() {
                let start = entry.start_sentence_idx;
                let end = if i + 1 < cm.map.len() {
                    cm.map[i + 1].start_sentence_idx.saturating_sub(1)
                } else {
                    total_sentences.saturating_sub(1)
                };
                let count = end.saturating_sub(start) + 1;

                let (v_recipe, is_max) = if entry.recipe.bas == u32::MAX {
                    ("MAX/MAX/MAX".to_string(), true)
                } else {
                    (format!("{}/{}/{}", entry.recipe.bas, entry.recipe.mod_v, entry.recipe.adv), false)
                };
                let l_recipe = format!("{:.2}/{:.2}/{:.2}",
                    entry.l_level_recipe.bas, entry.l_level_recipe.mod_v, entry.l_level_recipe.adv);

                // Mini bar chart from l_level_recipe
                let bar_html = if is_max {
                    "<span style='color:#a6adc8;font-size:0.8em'>N/A</span>".to_string()
                } else {
                    let total = entry.l_level_recipe.bas + entry.l_level_recipe.mod_v + entry.l_level_recipe.adv;
                    if total > 0.0 {
                        let w_bas = (entry.l_level_recipe.bas / total * 100.0) as u32;
                        let w_mod = (entry.l_level_recipe.mod_v / total * 100.0) as u32;
                        let w_adv = 100u32.saturating_sub(w_bas).saturating_sub(w_mod);
                        format!("<div class='bar-cell'><span class='bar bar-bas' style='width:{}px'></span><span class='bar bar-mod' style='width:{}px'></span><span class='bar bar-adv' style='width:{}px'></span></div>",
                            w_bas, w_mod, w_adv)
                    } else {
                        String::new()
                    }
                };

                html.push_str(&format!(
                    "<tr><td>{:.1}</td><td>S{}–S{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                    entry.level, start + 1, end + 1, count, v_recipe, l_recipe, bar_html));
            }

            html.push_str("</table></details>\n");
        }

        html.push_str("</body></html>");

        // Write to a temp file and open in browser
        let temp_dir = std::env::temp_dir();
        let html_path = temp_dir.join("weavelang_level_map.html");
        fs::write(&html_path, &html)
            .map_err(|e| format!("Failed to write HTML: {}", e))?;

        // Open in default browser
        opener::open(&html_path)
            .map_err(|e| format!("Failed to open browser: {}", e))?;

        Ok(format!("Level map opened in browser ({} levels, {} → {}).",
            sorted_keys.len(),
            sorted_keys.first().unwrap_or(&0),
            sorted_keys.last().unwrap_or(&0)))
    }

    fn execute_import_level_map(&mut self, path: &str) -> Result<String, String> {
        use crate::types::json_types::LevelMapFile;

        let resolved = self.resolve_path(path);
        let content = fs::read_to_string(&resolved)
            .map_err(|e| format!("Failed to read level map '{}': {}", resolved.display(), e))?;
        let lm_file: LevelMapFile = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse level map: {}", e))?;

        let level_count = lm_file.levels.len();
        self.state.book_map = Some(lm_file.levels);
        self.state.calibration_sentence_count = lm_file.meta.calibration_sentence_count;

        // Update project metadata from the level map if available
        if self.state.book_name.is_empty() {
            self.state.book_name = lm_file.meta.book_name.clone();
        }

        let cal_info = match lm_file.meta.calibration_sentence_count {
            Some(n) => format!(" (calibrated from {} sentences)", n),
            None => String::new(),
        };

        Ok(format!(
            "Imported level map from '{}' — {} levels (peak UL{}){}",
            path,
            level_count,
            lm_file.meta.natural_peak_level,
            cal_info,
        ))
    }

    fn execute_generate_weave(
        &self,
        level_arg: &str,
        chapter_range: Option<(usize, usize)>,
        frontier_config: crate::corpus_generator::FrontierRunConfig,
    ) -> Result<String, String> {
        use crate::domain::bridge::domain_sentences_to_json_chapter;
        use crate::simulation::dictionary::GlobalLemmaDictionary;
        use crate::simulation::metrics::TextMetrics;
        use crate::simulation::preprocessor;
        use crate::corpus_generator;
        use crate::simulation::text_generator;

        if self.state.document.is_empty() {
            return Err("No document loaded.".to_string());
        }

        if !self.state.audit_passed {
            return Err("Please run 'audit' on the project before outputting woven text.".to_string());
        }

        let output_dir = self.state.output_dir.as_ref()
            .ok_or("Output directory not set. Use 'set output_dir <path>' first.")?;
        let output_path = PathBuf::from(output_dir);

        // Create a book subdirectory to keep each book's files fenced
        let book_dir = if self.state.book_name.is_empty() {
            output_path.clone()
        } else {
            let sanitized = self.state.book_name.replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && c != ' ', "");
            let dir_name = sanitized.trim().replace(' ', "_");
            output_path.join(&dir_name)
        };

        // In chapter mode, output goes into a chapter subdirectory
        let (weave_dir, chapter_name_sanitized) = if let Some((ch_start, ch_end)) = chapter_range {
            // Find the chapter by its range to get the name
            let ch_name = self.state.chapters.iter()
                .find(|c| c.start == ch_start && c.end == ch_end)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("ch_{}-{}", ch_start, ch_end));
            let ch_dir_name = Self::sanitize_name(&ch_name);
            let ch_dir = book_dir.join(&ch_dir_name);
            (ch_dir, Some(ch_dir_name))
        } else {
            (book_dir.join("whole_book"), None)
        };

        // Put tts files in tts_files/ subdirectory (both chapter and whole-book modes)
        let tts_dir = weave_dir.join("tts_files");

        if !tts_dir.exists() {
            fs::create_dir_all(&tts_dir)
                .map_err(|e| format!("Failed to create output directory '{}': {}", tts_dir.display(), e))?;
        }

        let book_map = self.state.book_map.as_ref()
            .ok_or("No level map loaded. Use 'import level_map <path>' first.")?;

        // Build JsonChapter from domain sentences.
        // In chapter mode, pass only the chapter's sentences so the numerical
        // chapter has a 1:1 index alignment with the chapter range.
        let (base_lang, target_lang) = &self.state.project_languages;
        let ch_offset: usize = chapter_range.map(|(s, _)| s.saturating_sub(1)).unwrap_or(0);
        let doc_total = self.state.document.len();
        let sentences_for_chapter: &[crate::domain::sentence::Sentence] = if let Some((cs, ce)) = chapter_range {
            let s0 = cs.saturating_sub(1);
            let e0 = (ce.saturating_sub(1)).min(self.state.document.len().saturating_sub(1));
            &self.state.document[s0..=e0]
        } else {
            &self.state.document
        };
        let json_chapter = domain_sentences_to_json_chapter(
            sentences_for_chapter,
            &self.state.book_name,
            base_lang,
            target_lang,
            self.state.book_map.as_ref(),
        );

        // Build NumericalChapter + dictionary
        let mut dictionary = GlobalLemmaDictionary::new();
        let (numerical_chapter, _eng_word_counts) =
            preprocessor::json_chapter_to_numerical(&json_chapter, &mut dictionary);

        let ch_len = numerical_chapter.sentences_numerical.len();
        let mut generated_files: Vec<String> = Vec::new();
        let analysis_path = weave_dir.join("analysis.txt");

        // Helper: build filename from a level suffix string
        let build_file_name = |suffix: &str| -> String {
            if self.state.book_name.is_empty() {
                match &chapter_name_sanitized {
                    Some(ch) => format!("{}_{}.txt", ch, suffix),
                    None => format!("{}.txt", suffix),
                }
            } else {
                let sanitized = self.state.book_name.replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && c != ' ', "");
                let prefix = sanitized.trim().replace(' ', "_");
                match &chapter_name_sanitized {
                    Some(ch) => format!("{}_{}_{}.txt", prefix, ch, suffix),
                    None => format!("{}_{}.txt", prefix, suffix),
                }
            }
        };

        // Deterministic seed mixing for frontier boundary runs.
        let compose_frontier_seed = |level_value: u32, boundary_index: usize| -> u64 {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let mut hasher = DefaultHasher::new();
            self.state.book_name.hash(&mut hasher);
            chapter_name_sanitized.hash(&mut hasher);
            level_value.hash(&mut hasher);
            boundary_index.hash(&mut hasher);
            let mix = hasher.finish();
            frontier_config.seed ^ mix
        };

        // --- Helper: generate a flat-recipe output file ---
        let generate_flat = |bas: u32, mod_v: u32, adv: u32, suffix: &str,
                             generated: &mut Vec<String>| -> Result<(), String> {
            use crate::simulation::calibrator;

            let result = corpus_generator::generate_book_instance(
                &numerical_chapter,
                &json_chapter,
                &dictionary,
                bas, mod_v, adv,
                0.5,
                false,
            ).map_err(|e| format!("Generation failed for '{}': {}", suffix, e))?;

            let cleaned_parts: Vec<String> = result.final_text_parts
                .iter()
                .map(|p| text_generator::clean_text_for_tts(p))
                .collect();
            let output_text = cleaned_parts.join("\n\n");

            let metrics = TextMetrics::new(
                &result.all_output_lemma_instances,
                result.total_base_words,
            );
            let avd_score = metrics.calculate_avd_score();

            // For special tiers (ULb, ULm, ULa), append the derived user level
            let actual_suffix = if suffix.starts_with("UL") && suffix.len() == 3
                && ["ULb", "ULm", "ULa", "ULi"].contains(&suffix)
            {
                let ul = calibrator::get_user_level_from_avd(avd_score).round() as u32;
                format!("{}{}", suffix, ul)
            } else {
                suffix.to_string()
            };

            let file_name = build_file_name(&actual_suffix);
            let file_path = tts_dir.join(&file_name);
            fs::write(&file_path, &output_text)
                .map_err(|e| format!("Failed to write '{}': {}", file_path.display(), e))?;

            let recipe_obj = crate::simulation::numerical_types::VLevelRecipe { bas, mod_v, adv };
            corpus_generator::log_analysis_to_file(
                &analysis_path,
                &file_name,
                &result,
                avd_score,
                Some(recipe_obj.clone()),
                Some(recipe_obj),
                None,
                None,
                Some(&frontier_config),
                None,
                None,
            ).map_err(|e| format!("Failed to write analysis: {}", e))?;

            generated.push(format!("{} ({} sentences)", actual_suffix, result.final_text_parts.len()));
            Ok(())
        };

        // --- Helper: generate interlinear output file ---
        let generate_interlinear = |generated: &mut Vec<String>| -> Result<(), String> {
            let result_basic = corpus_generator::generate_book_instance(
                &numerical_chapter,
                &json_chapter,
                &dictionary,
                u32::MAX, 0, 0,
                0.5,
                false,
            ).map_err(|e| format!("Generation failed for interlinear (basic): {}", e))?;

            let result_base = corpus_generator::generate_book_instance(
                &numerical_chapter,
                &json_chapter,
                &dictionary,
                0, 0, 0,
                0.5,
                false,
            ).map_err(|e| format!("Generation failed for interlinear (base): {}", e))?;

            let count = result_basic.final_text_parts.len().min(result_base.final_text_parts.len());
            let mut interlinear_parts: Vec<String> = Vec::with_capacity(count * 3);
            for idx in 0..count {
                let basic_clean = text_generator::clean_text_for_tts(&result_basic.final_text_parts[idx]);
                let base_clean = text_generator::clean_text_for_tts(&result_base.final_text_parts[idx]);
                interlinear_parts.push(basic_clean.clone());
                interlinear_parts.push(base_clean);
                interlinear_parts.push(basic_clean);
            }
            let output_text = interlinear_parts.join("\n\n");

            let metrics = TextMetrics::new(
                &result_basic.all_output_lemma_instances,
                result_basic.total_base_words,
            );
            let avd_score = metrics.calculate_avd_score();

            // Derive user level from AVD and append to suffix
            let ul = crate::simulation::calibrator::get_user_level_from_avd(avd_score).round() as u32;
            let suffix = format!("ULi{}", ul);
            let file_name = build_file_name(&suffix);
            let file_path = tts_dir.join(&file_name);
            fs::write(&file_path, &output_text)
                .map_err(|e| format!("Failed to write '{}': {}", file_path.display(), e))?;

            let recipe_obj = crate::simulation::numerical_types::VLevelRecipe { bas: u32::MAX, mod_v: 0, adv: 0 };
            corpus_generator::log_analysis_to_file(
                &analysis_path,
                &file_name,
                &result_basic,
                avd_score,
                Some(recipe_obj.clone()),
                Some(recipe_obj),
                None,
                None,
                Some(&frontier_config),
                None,
                None,
            ).map_err(|e| format!("Failed to write analysis: {}", e))?;

            generated.push(format!("{} ({} sentences x3)", suffix, count));
            Ok(())
        };

        // --- Helper: generate raw source output file (ULr) ---
        let generate_raw_source = |generated: &mut Vec<String>| -> Result<(), String> {
            let mut source_parts: Vec<String> = Vec::with_capacity(sentences_for_chapter.len());
            for sent in sentences_for_chapter.iter() {
                let source_text = sent
                    .get_tier("base")
                    .or_else(|| sent.get_tier("basic_base"))
                    .map(|t| t.full_text())
                    .unwrap_or_default();
                let cleaned = text_generator::clean_text_for_tts(&source_text);
                source_parts.push(cleaned);
            }

            let output_text = source_parts.join("\n\n");
            let suffix = "ULr";
            let file_name = build_file_name(suffix);
            let file_path = tts_dir.join(&file_name);
            fs::write(&file_path, &output_text)
                .map_err(|e| format!("Failed to write '{}': {}", file_path.display(), e))?;

            generated.push(format!("{} ({} sentences)", suffix, source_parts.len()));
            Ok(())
        };

        // --- Dispatch: special modes b/m/a/i/r, or standard levels / all ---
        let is_all = level_arg == "all";

        match level_arg {
            "b" => {
                generate_flat(u32::MAX, 0, 0, "ULb", &mut generated_files)?;
            }
            "m" => {
                generate_flat(u32::MAX, u32::MAX, 0, "ULm", &mut generated_files)?;
            }
            "a" => {
                generate_flat(u32::MAX, u32::MAX, u32::MAX, "ULa", &mut generated_files)?;
            }
            "i" => {
                generate_interlinear(&mut generated_files)?;
            }
            "r" => {
                generate_raw_source(&mut generated_files)?;
            }
            _ => {
                // Standard level modes: numeric level or 'all'
                let levels: Vec<u32> = if is_all {
                    let mut lvls: Vec<u32> = book_map.keys()
                        .filter_map(|k| k.parse::<u32>().ok())
                        .collect();
                    lvls.sort();
                    lvls
                } else {
                    let lvl = level_arg.parse::<u32>()
                        .map_err(|_| format!("Invalid level '{}'. Use a number, 'all', 'b', 'm', 'a', 'i', or 'r'.", level_arg))?;
                    vec![lvl]
                };

                for level in &levels {
            // Level 0 is a special "acclimatization" level: 100% base language (recipe 0/0/0).
            // It has no entry in the book_map, so handle it via generate_flat.
            if *level == 0 {
                generate_flat(0, 0, 0, "UL0", &mut generated_files)?;
                continue;
            }

            // Level-shift for frontier mode: use recipe from level-1.
            // Level 1 + frontier → synthetic 0,0,0 recipe (all base language as starting point).
            // Level N>1 + frontier → look up recipe from level N-1.
            // Frontier disabled → use requested level as-is.
            let cm_owned_shift: Option<crate::types::json_types::JsonCurriculumMap> =
                if frontier_config.enabled && *level == 1 {
                    Some(crate::types::json_types::JsonCurriculumMap {
                        end_level: 1.0,
                        map: vec![crate::types::json_types::JsonCurriculumMapEntry {
                            level: 0.0,
                            start_sentence_idx: 0,
                            recipe: crate::simulation::numerical_types::VLevelRecipe {
                                bas: 0, mod_v: 0, adv: 0,
                            },
                            l_level_recipe: Default::default(),
                            target_avd: 0.0,
                            actual_avd: 0.0,
                        }],
                    })
                } else {
                    None
                };
            let cm: &crate::types::json_types::JsonCurriculumMap =
                if let Some(ref owned) = cm_owned_shift {
                    owned
                } else if frontier_config.enabled && *level > 1 {
                    let shifted_key = (*level - 1).to_string();
                    book_map.get(&shifted_key)
                        .ok_or(format!(
                            "No recipe found for shifted level {} (frontier mode, requested level {})",
                            level - 1,
                            level
                        ))?
                } else {
                    let level_key = level.to_string();
                    book_map.get(&level_key)
                        .ok_or(format!("No recipe found for level {}", level))?
                };
            if cm.map.is_empty() {
                return Err(format!("Empty curriculum map for level {}", level));
            }
            let first_entry = &cm.map[0];

            // --- Recipe selection ---
            // In chapter mode, use the first recipe of the requested level
            // so every chapter gets the true level-N recipe regardless of
            // its position in the book.  In full-book mode, step through
            // the progressive recipe map as before.
            let mut full_result = corpus_generator::BookGenerationResult::default();
            let mut boundary_prepass_metrics: Vec<corpus_generator::BoundaryPrepassMetrics> =
                Vec::new();
            let mut boundary_frontier_diags: Vec<corpus_generator::FrontierDiagnostics> =
                Vec::new();

            if chapter_range.is_some() {
                // Chapter mode: single recipe for the whole chapter.
                let recipe = &first_entry.recipe;
                if frontier_config.enabled {
                    let (total_tokens, unknown_tokens) =
                        corpus_generator::compute_prepass_metrics_for_slice(
                            &numerical_chapter,
                            &json_chapter,
                            &dictionary,
                            recipe.bas,
                            recipe.mod_v,
                            recipe.adv,
                            0.5,
                        )
                        .map_err(|e| format!("Pre-pass failed for level {}: {}", level, e))?;
                    boundary_prepass_metrics.push(corpus_generator::BoundaryPrepassMetrics {
                        boundary_index: 1,
                        sentence_start_1_based: 1,
                        sentence_end_1_based_inclusive: numerical_chapter
                            .sentences_numerical
                            .len(),
                        total_tokens,
                        unknown_tokens,
                    });
                }

                let frontier_slice_cfg = if frontier_config.enabled {
                    let m = boundary_prepass_metrics.first().cloned().ok_or_else(|| {
                        format!("Missing pre-pass metrics for level {} chapter run", level)
                    })?;
                    Some(corpus_generator::FrontierSliceConfig {
                        target_pct: frontier_config.target_pct,
                        expected_unknown_pct: m.expected_unknown_pct(),
                        total_tokens: m.total_tokens,
                        seed: compose_frontier_seed(*level, 1),
                    })
                } else {
                    None
                };

                let result = corpus_generator::generate_book_instance_with_frontier(
                    &numerical_chapter,
                    &json_chapter,
                    &dictionary,
                    recipe.bas,
                    recipe.mod_v,
                    recipe.adv,
                    0.5,
                    false,
                    frontier_slice_cfg.as_ref(),
                ).map_err(|e| format!("Generation failed for level {}: {}", level, e))?;

                if let Some(d) = result.frontier_diagnostics.clone() {
                    boundary_frontier_diags.push(d);
                }
                full_result.final_text_parts = result.final_text_parts;
                full_result.all_output_lemma_instances = result.all_output_lemma_instances;
                full_result.total_target_words = result.total_target_words;
                full_result.total_base_words = result.total_base_words;
                full_result.level_stats = result.level_stats;
                full_result.segment_stats = result.segment_stats;
            } else {
            // Full-book mode: progressive recipe stepping.
            for (i, entry) in cm.map.iter().enumerate() {
                let abs_start = entry.start_sentence_idx;
                let abs_end = if i + 1 < cm.map.len() {
                    cm.map[i + 1].start_sentence_idx
                } else {
                    doc_total
                };
                if abs_start >= abs_end {
                    continue;
                }

                // Clip the absolute range to the chapter bounds
                let clip_start = abs_start.max(ch_offset);
                let clip_end = abs_end.min(ch_offset + ch_len);
                if clip_start >= clip_end {
                    continue;
                }

                // Convert to chapter-relative indices
                let rel_start = clip_start - ch_offset;
                let rel_end = clip_end - ch_offset;

                let mut numerical_slice = numerical_chapter.clone();
                numerical_slice.sentences_numerical =
                    numerical_chapter.sentences_numerical[rel_start..rel_end].to_vec();

                let mut json_slice = json_chapter.clone();
                json_slice.content_blocks = json_chapter
                    .content_blocks
                    .iter()
                    .filter_map(|cb| match cb {
                        crate::JsonContentBlock::Sentence(s) => {
                            if numerical_slice
                                .sentences_numerical
                                .iter()
                                .any(|ns| ns.sentence_id_str == s.s_id)
                            {
                                Some(crate::JsonContentBlock::Sentence(s.clone()))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    })
                    .collect();

                if frontier_config.enabled {
                    let (total_tokens, unknown_tokens) =
                        corpus_generator::compute_prepass_metrics_for_slice(
                            &numerical_slice,
                            &json_slice,
                            &dictionary,
                            entry.recipe.bas,
                            entry.recipe.mod_v,
                            entry.recipe.adv,
                            0.5,
                        )
                        .map_err(|e| format!("Pre-pass failed for level {}: {}", level, e))?;
                    boundary_prepass_metrics.push(corpus_generator::BoundaryPrepassMetrics {
                        boundary_index: i + 1,
                        sentence_start_1_based: rel_start + 1,
                        sentence_end_1_based_inclusive: rel_end,
                        total_tokens,
                        unknown_tokens,
                    });
                }

                let frontier_slice_cfg = if frontier_config.enabled {
                    let expected_unknown_pct = boundary_prepass_metrics
                        .last()
                        .map(|m| m.expected_unknown_pct())
                        .unwrap_or(0.0);
                    let total_tokens = boundary_prepass_metrics
                        .last()
                        .map(|m| m.total_tokens)
                        .unwrap_or(0);
                    Some(corpus_generator::FrontierSliceConfig {
                        target_pct: frontier_config.target_pct,
                        expected_unknown_pct,
                        total_tokens,
                        seed: compose_frontier_seed(*level, i + 1),
                    })
                } else {
                    None
                };

                let slice_result = corpus_generator::generate_book_instance_with_frontier(
                    &numerical_slice,
                    &json_slice,
                    &dictionary,
                    entry.recipe.bas,
                    entry.recipe.mod_v,
                    entry.recipe.adv,
                    0.5, // inverse_diglot_threshold
                    false, // debug_markers
                    frontier_slice_cfg.as_ref(),
                ).map_err(|e| format!("Generation failed for level {}: {}", level, e))?;

                if let Some(d) = slice_result.frontier_diagnostics.clone() {
                    boundary_frontier_diags.push(d);
                }
                full_result.final_text_parts.extend(slice_result.final_text_parts);
                full_result.all_output_lemma_instances.extend(slice_result.all_output_lemma_instances);
                full_result.total_target_words += slice_result.total_target_words;
                full_result.total_base_words += slice_result.total_base_words;
                for (lvl, count) in slice_result.level_stats {
                    *full_result.level_stats.entry(lvl).or_insert(0) += count;
                }
                for (seg_type, count) in slice_result.segment_stats {
                    *full_result.segment_stats.entry(seg_type).or_insert(0) += count;
                }
            }
            } // end else (full-book mode)

            // Assemble output text: join sentence texts with double newline
            let cleaned_parts: Vec<String> = full_result.final_text_parts
                .iter()
                .map(|p| text_generator::clean_text_for_tts(p))
                .collect();
            let output_text = cleaned_parts.join("\n\n");

            // Determine filename.
            // In chapter mode the level is always the true requested level.
            // In full-book mode, the range may span multiple levels.
            let level_suffix = if chapter_range.is_some() {
                format!("UL{}", level)
            } else {
                let end_level_for_range = (cm.end_level - 1.0).floor() as u32;
                if end_level_for_range > *level {
                    format!("UL{}-{}", level, end_level_for_range)
                } else {
                    format!("UL{}", level)
                }
            };
            let file_name = build_file_name(&level_suffix);
            let file_path = tts_dir.join(&file_name);
            fs::write(&file_path, &output_text)
                .map_err(|e| format!("Failed to write '{}': {}", file_path.display(), e))?;

            // Compute AVD and log analysis profile
            let metrics = TextMetrics::new(
                &full_result.all_output_lemma_instances,
                full_result.total_base_words,
            );
            let avd_score = metrics.calculate_avd_score();

            let last_entry = cm.map.last();
            corpus_generator::log_analysis_to_file(
                &analysis_path,
                &file_name,
                &full_result,
                avd_score,
                Some(first_entry.recipe.clone()),
                last_entry.map(|e| e.recipe.clone()),
                Some(first_entry.l_level_recipe.clone()),
                last_entry.map(|e| e.l_level_recipe.clone()),
                Some(&frontier_config),
                if boundary_prepass_metrics.is_empty() {
                    None
                } else {
                    Some(&boundary_prepass_metrics)
                },
                if boundary_frontier_diags.is_empty() {
                    None
                } else {
                    Some(&boundary_frontier_diags)
                },
            ).map_err(|e| format!("Failed to write analysis: {}", e))?;

            generated_files.push(format!("{} ({} sentences)", level_suffix, full_result.final_text_parts.len()));

            if !boundary_frontier_diags.is_empty() {
                for (i, d) in boundary_frontier_diags.iter().enumerate() {
                    let realized_pct = if d.total_tokens > 0 {
                        (d.emitted_frontier_tokens as f32 / d.total_tokens as f32) * 100.0
                    } else {
                        0.0
                    };
                    let b_label = if boundary_frontier_diags.len() == 1 {
                        String::new()
                    } else {
                        format!("B#{:02} ", i + 1)
                    };
                    generated_files.push(format!(
                        "  [frontier] {}target={:.1}% realized={:.1}% emitted={}/{} tokens deck={} pass={} steered={}",
                        b_label,
                        d.target_pct,
                        realized_pct,
                        d.emitted_frontier_tokens,
                        d.target_frontier_tokens,
                        d.deck_size,
                        d.pass_count,
                        d.steering_adjustment_count,
                    ));
                }
            }
        }

                // When generating 'all', also produce UL0 and the 4 special outputs
                if is_all {
                    generate_flat(0, 0, 0, "UL0", &mut generated_files)?;
                    generate_flat(u32::MAX, 0, 0, "ULb", &mut generated_files)?;
                    generate_flat(u32::MAX, u32::MAX, 0, "ULm", &mut generated_files)?;
                    generate_flat(u32::MAX, u32::MAX, u32::MAX, "ULa", &mut generated_files)?;
                    generate_interlinear(&mut generated_files)?;
                    generate_raw_source(&mut generated_files)?;
                }
            } // end _ => (standard levels / all)
        } // end match

        Ok(format!(
            "Generated {} weave file(s) in '{}':\n  {}",
            generated_files.len(),
            tts_dir.display(),
            generated_files.join("\n  "),
        ))
    }

    /// Design Rule Check — returns a Vec of violation strings.
    /// Empty vec means PASS.
    /// When `range` is Some((start, end)) (0-based inclusive), only those
    /// sentences are checked.  Global rules are always checked.
    fn run_drc(&self, range: Option<(usize, usize)>) -> Vec<String> {
        use crate::domain::sentence::Sentence;
        use crate::domain::tier::TierState;

        let mut violations: Vec<String> = Vec::new();

        // Rule 8: Both project languages must be set
        let (base_lang, target_lang) = &self.state.project_languages;
        if base_lang.is_empty() || target_lang.is_empty() {
            violations.push("GLOBAL: project languages not set (use 'set languages <source> <target>')".to_string());
        }

        // Rule 9: Level map must be loaded
        let has_level_map = self.state.book_map.as_ref().map_or(false, |m| !m.is_empty());
        if !has_level_map {
            violations.push("GLOBAL: no level map loaded (use 'import level_map' or 'calibrate')".to_string());
        }

        let (range_start, range_end) = range.unwrap_or((0, self.state.document.len().saturating_sub(1)));
        for (i, sent) in self.state.document.iter().enumerate() {
            if i < range_start || i > range_end { continue; }
            let sn = i + 1; // 1-based for display
            let sid = &sent.id;

            // Rules 1-3: Check each WEAVE_TIER
            for &tid in Sentence::WEAVE_TIERS {
                match sent.tiers.get(tid) {
                    None => {
                        violations.push(format!("S{} ({}): tier '{}' is missing", sn, sid, tid));
                    }
                    Some(tier) => {
                        if tier.state != TierState::Valid {
                            violations.push(format!(
                                "S{} ({}): tier '{}' state is {:?}, expected Valid",
                                sn, sid, tid, tier.state
                            ));
                        }
                    }
                }
            }

            // Rules 4-5: Check mapping existence and completeness
            let fwd = sent.mappings.iter()
                .find(|m| m.from_tier_id == "basic_base" && m.to_tier_id == "basic_target");
            let inv = sent.mappings.iter()
                .find(|m| m.from_tier_id == "basic_target" && m.to_tier_id == "basic_base");

            // Rule 4: Forward mapping
            match fwd {
                None => {
                    violations.push(format!(
                        "S{} ({}): forward mapping (basic_base→basic_target) is missing",
                        sn, sid
                    ));
                }
                Some(mapping) => {
                    if mapping.entries.is_empty() {
                        violations.push(format!(
                            "S{} ({}): forward mapping has 0 entries",
                            sn, sid
                        ));
                    } else {
                        // Check word coverage
                        if let Some(tier) = sent.tiers.get("basic_base") {
                            if let Some(seg) = tier.segments.first() {
                                let word_count = seg.stream.words_enumerated().len();
                                let mapped_count = mapping.entries.len();
                                if mapped_count < word_count {
                                    violations.push(format!(
                                        "S{} ({}): forward mapping covers {}/{} words",
                                        sn, sid, mapped_count, word_count
                                    ));
                                }
                            }
                        }
                    }
                }
            }

            // Rule 5: Inverse mapping
            match inv {
                None => {
                    violations.push(format!(
                        "S{} ({}): inverse mapping (basic_target→basic_base) is missing",
                        sn, sid
                    ));
                }
                Some(mapping) => {
                    if mapping.entries.is_empty() {
                        violations.push(format!(
                            "S{} ({}): inverse mapping has 0 entries",
                            sn, sid
                        ));
                    } else {
                        // Check word coverage
                        if let Some(tier) = sent.tiers.get("basic_target") {
                            if let Some(seg) = tier.segments.first() {
                                let word_count = seg.stream.words_enumerated().len();
                                let mapped_count = mapping.entries.len();
                                if mapped_count < word_count {
                                    violations.push(format!(
                                        "S{} ({}): inverse mapping covers {}/{} words",
                                        sn, sid, mapped_count, word_count
                                    ));
                                }
                            }
                        }
                    }
                }
            }

            // Rule 6: Sentence-level — Advanced and Moderate segment counts must match
            let adv_seg_count = sent.tiers.get("advanced_target").map(|t| t.segments.len());
            let mod_seg_count = sent.tiers.get("moderate_target").map(|t| t.segments.len());
            if let (Some(a), Some(m)) = (adv_seg_count, mod_seg_count) {
                if a != m {
                    violations.push(format!(
                        "S{} ({}): advanced_target has {} segments but moderate_target has {} (must match)",
                        sn, sid, a, m
                    ));
                }
            }
        }

        violations
    }

    /// Run DRC filtered to a single tier across all (or a range of) sentences.
    /// Only checks whether the specified tier exists and is Valid on each sentence.
    fn run_drc_tier(&self, tier_id: &str, range: Option<(usize, usize)>) -> Vec<String> {
        use crate::domain::tier::TierState;

        let mut violations: Vec<String> = Vec::new();
        let (range_start, range_end) = range.unwrap_or((0, self.state.document.len().saturating_sub(1)));

        for (i, sent) in self.state.document.iter().enumerate() {
            if i < range_start || i > range_end { continue; }
            let sn = i + 1;
            let sid = &sent.id;

            match sent.tiers.get(tier_id) {
                None => {
                    violations.push(format!("S{} ({}): tier '{}' is missing", sn, sid, tier_id));
                }
                Some(tier) => {
                    if tier.state != TierState::Valid {
                        let label = match tier.state {
                            TierState::Dirty => "dirty",
                            TierState::Stale => "stale",
                            TierState::Pending => "pending",
                            TierState::Broken => "BROKEN",
                            TierState::Valid => unreachable!(),
                        };
                        violations.push(format!("S{} ({}): tier '{}' is {}", sn, sid, tier_id, label));
                    }
                }
            }
        }

        violations
    }

    /// Structural audit: walk all sentences and demote any tier that is
    /// currently Valid but violates a DRC rule.  Never promotes — only
    /// invalidates.  Returns a list of demotions performed.
    fn run_audit(&mut self) -> Vec<String> {
        use crate::domain::tier::TierState;

        let mut demotions: Vec<String> = Vec::new();

        for (idx, sent) in self.state.document.iter_mut().enumerate() {
            let sn = idx + 1;

            // Rule 1: basic_base Valid but forward mapping missing/incomplete → Broken
            if let Some(tier) = sent.tiers.get("basic_base") {
                if tier.state == TierState::Valid {
                    if !sent.check_mapping_coverage("basic_base") {
                        demotions.push(format!(
                            "S{}: basic_base demoted Valid → Broken (forward mapping incomplete)", sn
                        ));
                        if let Some(t) = sent.tiers.get_mut("basic_base") {
                            t.state = TierState::Broken;
                        }
                    }
                }
            }

            // Rule 2: basic_target Valid but inverse mapping missing/incomplete → Broken
            if let Some(tier) = sent.tiers.get("basic_target") {
                if tier.state == TierState::Valid {
                    if !sent.check_mapping_coverage("basic_target") {
                        demotions.push(format!(
                            "S{}: basic_target demoted Valid → Broken (inverse mapping incomplete)", sn
                        ));
                        if let Some(t) = sent.tiers.get_mut("basic_target") {
                            t.state = TierState::Broken;
                        }
                    }
                }
            }

            // Rule 3: moderate_target Valid but segment count ≠ advanced_target → Broken
            let adv_seg = sent.tiers.get("advanced_target").map(|t| t.segments.len());
            let mod_seg = sent.tiers.get("moderate_target").map(|t| t.segments.len());
            if let Some(tier) = sent.tiers.get("moderate_target") {
                if tier.state == TierState::Valid {
                    if let (Some(a), Some(m)) = (adv_seg, mod_seg) {
                        if a > 0 && m > 0 && a != m {
                            demotions.push(format!(
                                "S{}: moderate_target demoted Valid → Broken (has {} segments, advanced_target has {})",
                                sn, m, a
                            ));
                            if let Some(t) = sent.tiers.get_mut("moderate_target") {
                                t.state = TierState::Broken;
                            }
                        }
                    }
                }
            }

            // Rule 4: advanced_target Valid but no segments → Broken
            if let Some(tier) = sent.tiers.get("advanced_target") {
                if tier.state == TierState::Valid && tier.segments.is_empty() {
                    demotions.push(format!(
                        "S{}: advanced_target demoted Valid → Broken (no segments)", sn
                    ));
                    if let Some(t) = sent.tiers.get_mut("advanced_target") {
                        t.state = TierState::Broken;
                    }
                }
            }
        }

        // Mark audit as passed (even if demotions occurred — the point
        // is that the audit has run and structural state is now accurate).
        self.state.audit_passed = true;

        demotions
    }

    fn execute_calibrate(&mut self, max_level: Option<u32>) -> Result<String, String> {
        use crate::domain::bridge::domain_sentences_to_json_chapter;
        use crate::simulation::calibrator;
        use crate::simulation::frequency_manager;

        if self.state.document.is_empty() {
            return Err("No document loaded.".to_string());
        }

        // Ensure frequency list is loaded
        if frequency_manager::get_max_rank() == 0 {
            return Err("Frequency list not loaded. Cannot calibrate.".to_string());
        }

        // Resolve master AVD scale path from config
        let content_dir = self.state.config.as_ref()
            .map(|c| c.content_project_dir_path())
            .ok_or("No config loaded — cannot locate master AVD scale file.")?;
        let scale_path = content_dir.join("generated_profiles").join("master_avd_scale.csv");
        if !scale_path.exists() {
            return Err(format!(
                "Master AVD scale not found at '{}'. Run the AVD hunter first.",
                scale_path.display()
            ));
        }

        let master_scale = calibrator::parse_master_avd_scale(&scale_path)
            .map_err(|e| format!("Failed to read master AVD scale: {}", e))?;

        let max_level = max_level.unwrap_or(45);

        let (base_lang, target_lang) = &self.state.project_languages;

        // In chapter mode: only include sentences from valid (complete) chapters
        let sentences_for_calibration: &[crate::domain::sentence::Sentence];
        let chapter_mode_info: String;
        let owned_sentences: Vec<crate::domain::sentence::Sentence>;

        if self.state.chapter_mode && !self.state.chapters.is_empty() {
            // Collect valid chapters
            let valid_chapters: Vec<&crate::app::state::Chapter> = self.state.chapters.iter()
                .filter(|ch| self.chapter_is_valid(ch))
                .collect();

            if valid_chapters.is_empty() {
                return Err("Chapter mode: no chapters have all sentences complete. Complete at least one chapter before calibrating.".to_string());
            }

            // Build the full-length sentence vector with empty placeholders for gaps.
            // This preserves book-global sentence indexing.
            let total = self.state.document.len();
            let mut synthetic = Vec::with_capacity(total);
            let mut chapter_sentence_count = 0usize;

            for idx in 0..total {
                let in_valid_chapter = valid_chapters.iter().any(|ch| {
                    let s0 = ch.start.saturating_sub(1);
                    let e0 = ch.end.saturating_sub(1);
                    idx >= s0 && idx <= e0
                });
                if in_valid_chapter {
                    synthetic.push(self.state.document[idx].clone());
                    chapter_sentence_count += 1;
                } else {
                    // Empty placeholder — preserves indexing
                    synthetic.push(crate::domain::sentence::Sentence::new(
                        format!("__placeholder_{}", idx),
                    ));
                }
            }

            chapter_mode_info = format!(
                " (chapter mode: {} valid chapter(s), {} sentences of {} total)",
                valid_chapters.len(), chapter_sentence_count, total
            );
            owned_sentences = synthetic;
            sentences_for_calibration = &owned_sentences;
        } else {
            chapter_mode_info = String::new();
            owned_sentences = Vec::new(); // unused
            let _ = &owned_sentences; // suppress warning
            sentences_for_calibration = &self.state.document;
        }

        // Build JsonChapter from the selected sentences
        let json_chapter = domain_sentences_to_json_chapter(
            sentences_for_calibration,
            &self.state.book_name,
            base_lang,
            target_lang,
            None, // no existing level maps
        );

        // In chapter mode, pass the true total sentence count so the calibrator
        // can extrapolate word counts and pacing for the full book.
        let total_sentences_hint = if self.state.chapter_mode && !self.state.chapters.is_empty() {
            Some(self.state.document.len())
        } else {
            None
        };

        let curriculum_maps = calibrator::calibrate_from_chapter(
            &json_chapter,
            &master_scale,
            max_level,
            total_sentences_hint,
        ).map_err(|e| format!("Calibration failed: {}", e))?;

        let level_count = curriculum_maps.len();
        self.state.book_map = Some(curriculum_maps);

        // Track how many completed sentences were used for calibration
        let cal_sentence_count = if self.state.chapter_mode && !self.state.chapters.is_empty() {
            // chapter_sentence_count was computed above in the chapter_mode branch
            sentences_for_calibration.iter()
                .filter(|s| !s.id.starts_with("__placeholder_"))
                .count()
        } else {
            self.state.document.len()
        };
        self.state.calibration_sentence_count = Some(cal_sentence_count);

        Ok(format!(
            "Calibration complete — {} start-level maps generated and loaded{} ({} sentences used).",
            level_count, chapter_mode_info, cal_sentence_count
        ))
    }

    fn execute_debug_dump(&self, start: usize, end: usize, path: Option<&str>) -> Result<String, String> {
        if self.state.document.is_empty() {
            return Err("No document loaded.".to_string());
        }

        let max_idx = self.state.document.len().saturating_sub(1);
        let s = start.min(max_idx);
        let e = end.min(max_idx);
        let (s, e) = if s <= e { (s, e) } else { (e, s) };

        // Tier display order (matching the project's tier hierarchy)
        let tier_order = [
            ("base",             "Base (Original)"),
            ("advanced_target",  "Advanced Target"),
            ("moderate_target",  "Moderate Target"),
            ("basic_target",     "Basic Target"),
            ("basic_base",       "Basic Base (Simplified)"),
        ];

        let mut out = String::new();
        out.push_str(&format!("=== Debug Dump: sentences {} to {} ===\n", s + 1, e + 1));
        out.push_str(&format!("=== Book: {} | Languages: {}/{} ===\n\n",
            if self.state.book_name.is_empty() { "Unknown" } else { &self.state.book_name },
            self.state.project_languages.0,
            self.state.project_languages.1,
        ));

        for idx in s..=e {
            let sent = &self.state.document[idx];
            let base_text = sent.get_tier("base")
                .map(|t| t.full_text())
                .unwrap_or_else(|| "(no base tier)".to_string());

            out.push_str(&format!(
                "================================================================\n=== {} (sentence {}): \"{}\" ===\n================================================================\n\n",
                sent.id,
                idx + 1,
                if base_text.len() > 80 { format!("{}...", &base_text[..77]) } else { base_text }
            ));

            // Tiers
            for (tier_id, tier_label) in &tier_order {
                if let Some(tier) = sent.get_tier(tier_id) {
                    out.push_str(&format!("--- {} ({}) ---\n", tier_label, tier_id));
                    out.push_str(&format!("  Text: \"{}\"\n", tier.full_text()));
                    out.push_str(&format!("  State: {:?}\n", tier.state));

                    // Show segments if more than one
                    if tier.segments.len() > 1 {
                        out.push_str(&format!("  Segments ({}):\n", tier.segments.len()));
                        for (si, seg) in tier.segments.iter().enumerate() {
                            out.push_str(&format!("    [{}] \"{}\"\n", si, seg.full_text()));
                        }
                    }

                    // Show lemmas if present
                    if !tier.lemmas.is_empty() {
                        let display_count = tier.lemmas.len().min(20);
                        let ranked: Vec<String> = tier.lemmas[..display_count].iter()
                            .map(|l| match crate::simulation::frequency_manager::get_rank_for_lemma(l) {
                                Some(r) => format!("{}<{}>", l, r),
                                None => format!("{}<->", l),
                            }).collect();
                        out.push_str(&format!("  Lemmas ({}): {}", tier.lemmas.len(),
                            ranked.join(", ")));
                        if tier.lemmas.len() > 20 {
                            out.push_str(&format!(" ... (+{} more)", tier.lemmas.len() - 20));
                        }
                        out.push('\n');
                    }
                    out.push('\n');
                }
            }

            // Mappings
            if !sent.mappings.is_empty() {
                for mapping in &sent.mappings {
                    out.push_str(&format!("--- Mapping: {} → {} ({} entries) ---\n",
                        mapping.from_tier_id, mapping.to_tier_id, mapping.entries.len()));
                    for entry in &mapping.entries {
                        let viable_marker = if !entry.is_viable { " [NOT VIABLE]" } else { "" };
                        let proper_marker = if entry.is_proper_noun { " [PROPER]" } else { "" };
                        let ranked_lemmas: Vec<String> = entry.target_lemmas.iter()
                            .map(|l| match crate::simulation::frequency_manager::get_rank_for_lemma(l) {
                                Some(r) => format!("{}<{}>", l, r),
                                None => format!("{}<->", l),
                            }).collect();
                        out.push_str(&format!("  w{}: \"{}\" lemmas=[{}]{}{}\n",
                            entry.source_word_id.0,
                            entry.target_text,
                            ranked_lemmas.join(", "),
                            viable_marker,
                            proper_marker,
                        ));
                    }
                    out.push('\n');
                }
            } else {
                out.push_str("--- Mappings: (none) ---\n\n");
            }
        }

        // Write to file if path provided
        if let Some(file_path) = path {
            fs::write(file_path, &out)
                .map_err(|e| format!("Failed to write debug dump to '{}': {}", file_path, e))?;
            Ok(format!("Debug dump written to '{}' ({} sentences, {} bytes)",
                file_path, e - s + 1, out.len()))
        } else {
            Ok(out)
        }
    }

    // ----- Chapter Mode helpers -----

    /// Sanitize a name for use in file/directory names: replace spaces with underscores,
    /// strip non-alphanumeric chars (except _ and -).
    fn sanitize_name(name: &str) -> String {
        let sanitized = name.replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && c != ' ', "");
        sanitized.trim().replace(' ', "_")
    }

    fn execute_new_chapter(&mut self, name: String, start: usize, end: usize) -> Result<String, String> {
        if name.is_empty() {
            return Err("Chapter name cannot be empty.".to_string());
        }
        // Check for duplicate name
        if self.state.chapters.iter().any(|c| c.name == name) {
            return Err(format!("A chapter named '{}' already exists.", name));
        }
        // Check for overlapping ranges (1-based inclusive)
        for ch in &self.state.chapters {
            if start <= ch.end && end >= ch.start {
                return Err(format!(
                    "Range {}-{} overlaps with chapter '{}' ({}-{}).",
                    start, end, ch.name, ch.start, ch.end
                ));
            }
        }
        self.state.chapters.push(crate::app::state::Chapter {
            name: name.clone(),
            start,
            end,
        });
        // Sort by start index
        self.state.chapters.sort_by_key(|c| c.start);
        // Persist
        self.save_chapters();
        Ok(format!("Chapter '{}' created: sentences {}-{}.", name, start, end))
    }

    fn execute_list_chapters(&self) -> Result<String, String> {
        if self.state.chapters.is_empty() {
            return Ok("No chapters defined.".to_string());
        }
        let mut out = String::new();
        out.push_str(&format!("Chapters ({}):\n", self.state.chapters.len()));

        let selected_idx = self.state.selected_chapter_idx;

        for (i, ch) in self.state.chapters.iter().enumerate() {
            let marker = if selected_idx == Some(i) { ">" } else { " " };
            // Validity: every sentence in range has all 5 tiers + both mappings
            let valid = self.chapter_is_valid(ch);
            let status = if valid { "✓ valid" } else { "✗ incomplete" };
            let count = ch.end.saturating_sub(ch.start) + 1;
            out.push_str(&format!(
                "{} {:>2}. [{}] \"{}\" sentences {}-{} ({} sentences)\n",
                marker, i + 1, status, ch.name, ch.start, ch.end, count
            ));
        }
        Ok(out.trim_end().to_string())
    }

    fn execute_delete_chapter(&mut self, name: &str) -> Result<String, String> {
        let pos = self.state.chapters.iter().position(|c| c.name == name);
        match pos {
            Some(idx) => {
                self.state.chapters.remove(idx);
                // Fix up selected index
                if let Some(sel) = self.state.selected_chapter_idx {
                    if sel == idx {
                        self.state.selected_chapter_idx = None;
                    } else if sel > idx {
                        self.state.selected_chapter_idx = Some(sel - 1);
                    }
                }
                self.save_chapters();
                Ok(format!("Chapter '{}' deleted.", name))
            }
            None => Err(format!("No chapter named '{}'.", name)),
        }
    }

    fn execute_select_chapter(&mut self, name: &str) -> Result<String, String> {
        let pos = self.state.chapters.iter().position(|c| c.name == name);
        match pos {
            Some(idx) => {
                self.state.selected_chapter_idx = Some(idx);
                let ch = &self.state.chapters[idx];
                Ok(format!("Selected chapter '{}' (sentences {}-{}).", ch.name, ch.start, ch.end))
            }
            None => Err(format!("No chapter named '{}'.", name)),
        }
    }

    fn chapter_is_valid(&self, ch: &crate::app::state::Chapter) -> bool {
        // Convert 1-based inclusive range to 0-based
        let start_0 = ch.start.saturating_sub(1);
        let end_0 = ch.end.saturating_sub(1);
        for idx in start_0..=end_0 {
            if let Some(s) = self.state.document.get(idx) {
                if !s.is_weave_ready() {
                    return false;
                }
            } else {
                return false; // Out of range
            }
        }
        true
    }

    fn execute_init_media_workspace(&self) -> Result<String, String> {
        let output_dir = self.state.output_dir.as_ref()
            .ok_or("Output directory not set. Use 'set output_dir <path>' first.")?;
        let book_dir = crate::services::av_producer::AvProducer::resolve_book_dir(output_dir, &self.state.book_name);

        let subdirs = ["tts_files", "audio", "video", "illustrations"];

        // Create whole_book directory structure
        let whole_book_dir = book_dir.join("whole_book");
        for sub in &subdirs {
            let dir = whole_book_dir.join(sub);
            if !dir.exists() {
                fs::create_dir_all(&dir)
                    .map_err(|e| format!("Failed to create '{}': {}", dir.display(), e))?;
            }
        }
        // Also create chunks dir under audio
        let chunks_dir = whole_book_dir.join("audio").join("chunks");
        if !chunks_dir.exists() {
            fs::create_dir_all(&chunks_dir)
                .map_err(|e| format!("Failed to create '{}': {}", chunks_dir.display(), e))?;
        }

        let mut created_chapters = Vec::new();
        // Create per-chapter directory structures
        for ch in &self.state.chapters {
            let ch_dir_name = Self::sanitize_name(&ch.name);
            let ch_dir = book_dir.join(&ch_dir_name);
            for sub in &subdirs {
                let dir = ch_dir.join(sub);
                if !dir.exists() {
                    fs::create_dir_all(&dir)
                        .map_err(|e| format!("Failed to create '{}': {}", dir.display(), e))?;
                }
            }
            let ch_chunks_dir = ch_dir.join("audio").join("chunks");
            if !ch_chunks_dir.exists() {
                fs::create_dir_all(&ch_chunks_dir)
                    .map_err(|e| format!("Failed to create '{}': {}", ch_chunks_dir.display(), e))?;
            }
            created_chapters.push(ch_dir_name);
        }

        let mut msg = format!("Media workspace initialized at '{}'.\n  whole_book/", book_dir.display());
        for name in &created_chapters {
            msg.push_str(&format!("\n  {}/", name));
        }
        Ok(msg)
    }

    /// Save chapters to _chapters.toml in the project directory.
    fn save_chapters(&self) {
        if let Some(cfg) = &self.state.config {
            let chapters_path = PathBuf::from(&cfg.content_project_dir).join("_chapters.toml");
            // Use a wrapper struct for proper TOML serialization
            #[derive(serde::Serialize)]
            struct ChaptersFile<'a> {
                chapters: &'a [crate::app::state::Chapter],
            }
            let wrapper = ChaptersFile { chapters: &self.state.chapters };
            if let Ok(toml_str) = toml::to_string_pretty(&wrapper) {
                let _ = fs::write(&chapters_path, toml_str);
            }
        }
    }

    /// Load chapters from _chapters.toml in the project directory.
    pub fn load_chapters(&mut self) {
        self.state.chapters.clear();
        self.state.selected_chapter_idx = None;
        if let Some(cfg) = &self.state.config {
            let chapters_path = PathBuf::from(&cfg.content_project_dir).join("_chapters.toml");
            if chapters_path.exists() {
                if let Ok(content) = fs::read_to_string(&chapters_path) {
                    #[derive(serde::Deserialize)]
                    struct ChaptersFile {
                        #[serde(default)]
                        chapters: Vec<crate::app::state::Chapter>,
                    }
                    if let Ok(parsed) = toml::from_str::<ChaptersFile>(&content) {
                        self.state.chapters = parsed.chapters;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// AV Job background worker — reads stdout/stderr from the child process
// and pushes lines into the shared AvJobState.
// ---------------------------------------------------------------------------

fn av_job_reader(
    mut child: std::process::Child,
    job: std::sync::Arc<std::sync::Mutex<crate::app::state::AvJobState>>,
) {
    use std::io::{BufRead, BufReader};

    // Take stdout and stderr from the child
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Spawn a thread to read stderr, collecting into a shared buffer
    let stderr_lines: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let stderr_lines_clone = stderr_lines.clone();
    let job_clone = job.clone();
    let stderr_thread = stderr.map(|se| {
        std::thread::spawn(move || {
            let reader = BufReader::new(se);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        let trimmed = l.trim_end().to_string();
                        if !trimmed.is_empty() {
                            stderr_lines_clone.lock().unwrap().push(trimmed.clone());
                            job_clone.lock().unwrap().output_lines.push(trimmed);
                        }
                    }
                    Err(_) => break,
                }
            }
        })
    });

    // Read stdout on the current thread
    if let Some(so) = stdout {
        let reader = BufReader::new(so);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    let trimmed = l.trim_end().to_string();
                    if !trimmed.is_empty() {
                        job.lock().unwrap().output_lines.push(trimmed);
                    }
                }
                Err(_) => break,
            }
        }
    }

    // Wait for stderr thread
    if let Some(t) = stderr_thread {
        let _ = t.join();
    }

    // Wait for the child process to exit
    let status = child.wait();
    let mut j = job.lock().unwrap();
    j.child_pid = None;

    if j.cancel_requested {
        j.output_lines.push("--- Cancelled ---".to_string());
        j.result_message = Some("AV job cancelled.".to_string());
    } else {
        match status {
            Ok(s) if s.success() => {
                j.output_lines.push("--- Finished successfully ---".to_string());
                j.result_message = Some("AV job completed successfully.".to_string());
            }
            Ok(s) => {
                let code = s.code().map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string());
                j.output_lines.push(format!("--- Failed (exit code {}) ---", code));
                j.result_message = Some(format!("AV job failed with exit code {}.", code));
            }
            Err(e) => {
                j.output_lines.push(format!("--- Error waiting for process: {} ---", e));
                j.result_message = Some(format!("AV job error: {}", e));
            }
        }
    }
    j.finished = true;
}

/// Minimal HTML escaping for injecting text into generated HTML.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
