// src/gui/components/detail_view/text_view.rs

use crate::domain::tier::TierState;
use crate::domain::llm_log::LlmCallRecord;
use crate::gui::preview;
use crate::app::state::{AppState, TierView};
use crate::simulation::frequency_manager;
use eframe::egui;
use regex::Regex;

/// Maps a tier ID to the engine stage name used to generate it.
fn stage_for_tier(tier_id: &str) -> Option<&'static str> {
    match tier_id {
        "advanced_target" => Some("GenerateAdvancedTarget"),
        "moderate_target" => Some("GenerateModerateTarget"),
        "basic_target"    => Some("GenerateBasicTarget"),
        "basic_base"      => Some("GenerateBasicBase"),
        _                 => None,
    }
}

#[allow(dead_code)]
fn extract_translation_from_response(raw_response: &str, target_id: &str) -> String {
    let num_str: String = target_id.chars().filter(|c| c.is_ascii_digit()).collect();

    let pattern_str = format!(r"(?mi)^(?:id\s*)?S0*{num_str}\s*[:.]\s*(.*)$");
    if let Ok(specific_re) = Regex::new(&pattern_str) {
        for line in raw_response.lines() {
            let trimmed = line.trim();
            if let Some(caps) = specific_re.captures(trimmed) {
                return caps.get(1).unwrap().as_str().trim().to_string();
            }
        }
    }

    let generic_re = Regex::new(r"(?mi)^(?:id\s*)?S\d+\s*[:.]\s*(.*)$").unwrap();
    if let Some(caps) = generic_re.captures(raw_response) {
        return caps.get(1).unwrap().as_str().trim().to_string();
    }

    let non_empty_lines: Vec<&str> = raw_response
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    if let Some(last) = non_empty_lines.last() {
        return last.trim().to_string();
    }

    raw_response.trim().to_string()
}

pub fn render(ui: &mut egui::Ui, view: TierView, state: &mut AppState) {
    if view == TierView::Simulation {
        ui.heading("Tier: Live Simulation");
        ui.separator();
        if let Some(sentence) = state.get_current_sentence() {
            let recipe = state.get_effective_recipe();
            let mut text = preview::generate_preview_text(sentence, &recipe);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .font(egui::TextStyle::Body)
                            .desired_width(f32::INFINITY)
                            .frame(false),
                    );
                });
        } else {
            ui.label("No sentence selected.");
        }
        return;
    }

    // --- Configuration for this View ---
    let (tier_id, _lang_code, parent_tier_id, prompt_name) = match view {
        // Base is the Root.
        TierView::Base => ("base", "en", None, None),

        // Advanced depends on Base (Translation)
        TierView::AdvancedTarget => (
            "advanced_target",
            "es",
            Some("base"),
            Some("translate_text"),
        ),

        // Moderate depends on Advanced (Simplification)
        TierView::ModerateTarget => (
            "moderate_target",
            "es",
            Some("advanced_target"),
            Some("simplify_segments_moderate"),
        ),

        // Basic Target depends on Moderate (Simplification)
        TierView::BasicTarget => (
            "basic_target",
            "es",
            Some("moderate_target"),
            Some("simplify_segments_basic"),
        ),

        // --- FIXED: Basic Base depends on Base (Simplification of the original English) ---
        TierView::BasicBase => (
            "basic_base",
            "en",
            Some("base"),
            Some("simplify_to_basic_english"),
        ),

        TierView::Simulation => unreachable!(),
    };
    ui.horizontal(|ui| {
        ui.heading(format!("Tier: {tier_id}"));
        if let Some(sentence) = state.get_current_sentence() {
            if let Some(tier) = sentence.get_tier(tier_id) {
                match tier.state {
                    TierState::Valid => ui.colored_label(egui::Color32::DARK_GREEN, "[Valid]"),
                    TierState::Dirty => {
                        ui.colored_label(egui::Color32::from_rgb(200, 100, 0), "[Dirty]")
                    }
                    TierState::Stale => {
                        ui.colored_label(egui::Color32::from_rgb(255, 140, 0), "[Stale]")
                    }
                    TierState::Broken => ui.colored_label(egui::Color32::RED, "[BROKEN]"),
                };
            } else {
                ui.colored_label(egui::Color32::GRAY, "[Missing]");
            }
        }
    });
    ui.separator();

    let cur_selected_idx = state.selected_sentence_idx;

    // Snapshot the LLM log before taking a mutable borrow, so we can render
    // it afterwards without borrow conflicts.
    let llm_log: Vec<LlmCallRecord> = state
        .get_current_sentence()
        .and_then(|s| s.get_tier(tier_id))
        .map(|t| t.llm_log.clone())
        .unwrap_or_default();

    let mut parent_text_opt = None;
    if let Some(p_id) = parent_tier_id {
        if let Some(sentence) = state.get_current_sentence() {
            if let Some(parent_tier) = sentence.get_tier(p_id) {
                parent_text_opt = Some(parent_tier.full_text());
            }
        }
    }

    let mut current_tier_text = String::new();
    let mut current_tier_state = TierState::Valid;
    let mut current_tier_exists = false;
    let mut pending_action: Option<&str> = None;
    let mut pending_text_update: Option<String> = None;

    // Snapshot segment details for the segment breakdown display.
    // Includes lemma list for the segment-focused view in Adv/Mod tiers.
    let segment_details: Vec<(String, String, Vec<String>)> = state
        .get_current_sentence()
        .and_then(|s| s.get_tier(tier_id))
        .map(|t| {
            t.segments
                .iter()
                .map(|seg| (seg.id.clone(), seg.full_text(), seg.lemmas.clone()))
                .collect()
        })
        .unwrap_or_default();

    // Determine if this is a segment-focused tier (Adv/Mod)
    let is_segment_focused = matches!(view, TierView::AdvancedTarget | TierView::ModerateTarget);
    // Determine if this is a mapping-focused tier (Bas B / Bas T)
    let is_mapping_focused = matches!(view, TierView::BasicBase | TierView::BasicTarget);

    // ── Snapshot mapping table rows for Bas B / Bas T ────────────────────
    // Each row: (is_word, word_idx_1based, bg_ref_word_idx, source_text, target_text, lemma_display)
    // For background rows: bg_ref_word_idx = the 1-based index of the word after this background
    //   (used for edit_b command). 0 means trailing background.
    // For word rows: bg_ref_word_idx is unused (0).
    let mapping_rows: Vec<(bool, usize, usize, String, String, String)> = if is_mapping_focused {
        // Determine mapping direction:
        // Bas B: source=basic_base, target=basic_target  (forward diglot)
        // Bas T: source=basic_target, target=basic_base  (inverse diglot)
        let (map_source, map_target) = if tier_id == "basic_base" {
            ("basic_base", "basic_target")
        } else {
            ("basic_target", "basic_base")
        };

        state.get_current_sentence().map(|sent| {
            let tier = match sent.get_tier(tier_id) {
                Some(t) => t,
                None => return Vec::new(),
            };
            let seg = match tier.segments.first() {
                Some(s) => s,
                None => return Vec::new(),
            };

            // Build mapping lookup
            let mapping_lookup: std::collections::HashMap<crate::domain::primitives::WordId, &crate::domain::mapping::MappingEntry> = sent
                .mappings()
                .iter()
                .find(|m| m.from_tier_id == map_source && m.to_tier_id == map_target)
                .map(|tier_mapping| {
                    tier_mapping.entries.iter().map(|e| (e.source_word_id, e)).collect()
                })
                .unwrap_or_default();

            let mut rows = Vec::new();
            let mut word_idx = 0usize;

            for tok in seg.stream.tokens() {
                match tok {
                    crate::domain::token_stream::Token::Background(bg) => {
                        let display = bg.clone();
                        // bg_ref_idx: the next word's 1-based index
                        let bg_ref = word_idx + 1;
                        rows.push((false, 0, bg_ref, display, String::new(), String::new()));
                    }
                    crate::domain::token_stream::Token::Word(w) => {
                        word_idx += 1;
                        let (target, lemma_display) = if let Some(entry) = mapping_lookup.get(&w.id) {
                            let lemmas_str: Vec<String> = entry.target_lemmas.iter().map(|l| {
                                match frequency_manager::get_rank_for_lemma(l) {
                                    Some(r) => format!("{}<{}>", l, r),
                                    None => format!("{}<?>", l),
                                }
                            }).collect();
                            (entry.target_text.clone(), lemmas_str.join(", "))
                        } else {
                            (String::new(), String::new())
                        };
                        rows.push((true, word_idx, 0, w.text.clone(), target, lemma_display));
                    }
                }
            }
            rows
        }).unwrap_or_default()
    } else {
        Vec::new()
    };

    // ── 1. Snapshot tier info (immutable) ────────────────────────────────────
    if let Some(sentence) = state.get_current_sentence() {
        if let Some(tier) = sentence.get_tier(tier_id) {
            current_tier_text = tier.full_text();
            current_tier_state = tier.state;
            current_tier_exists = true;
        }
    }

    if current_tier_exists {
        // ── 2. Render buttons ────────────────────────────────────────────
        ui.horizontal(|ui| {
            if current_tier_state == TierState::Dirty {
                ui.label(egui::RichText::new("Changes detected.").italics());
                if ui.button("⟳ Apply, Re-tokenize & Validate").clicked() {
                    pending_action = Some("Retokenize");
                }
            } else {
                if parent_tier_id.is_some()
                    && prompt_name.is_some()
                    && parent_text_opt.is_some()
                {
                    let btn_label = if is_mapping_focused {
                        "🧠 Generate Sentence"
                    } else {
                        "🧠 Regenerate via LLM"
                    };
                    if ui.button(btn_label).clicked() {
                        pending_action = Some("Regenerate");
                    }
                }
                if ui.button("⟳ Force Re-tokenize").clicked() {
                    pending_action = Some("Retokenize");
                }
            }

            // Allow regenerating even if dirty
            if current_tier_state == TierState::Dirty
                && parent_tier_id.is_some()
                && prompt_name.is_some()
                && parent_text_opt.is_some()
            {
                let btn_label = if is_mapping_focused {
                    "🧠 Generate Sentence (Discard Edits)"
                } else {
                    "🧠 Regenerate (Discard Edits)"
                };
                if ui.button(btn_label).clicked() {
                    pending_action = Some("Regenerate");
                }
            }

            // Approve button — available when dirty or stale
            if current_tier_state == TierState::Dirty || current_tier_state == TierState::Stale {
                if ui.button("✓ Approve").on_hover_text("Lemmatize + validate this tier").clicked() {
                    pending_action = Some("Approve");
                }
            }
        });

        if current_tier_state != TierState::Valid {
            ui.separator();
        }

        // ── 3. Render editor area ────────────────────────────────────────
        let text_max_h = if llm_log.is_empty() { f32::INFINITY } else { 240.0 };

        if is_segment_focused {
            // ── Segment-focused view (Adv / Mod) ─────────────────────────
            // Always update edit buffers with the latest segment data from engine state
            for (seg_id, seg_text, _) in &segment_details {
                let buf_key = format!("{}_{}_{}", tier_id, seg_id, cur_selected_idx);
                state.seg_edit_buffers.insert(buf_key, seg_text.clone());
            }

            let mut seg_commands: Vec<String> = Vec::new();

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(text_max_h)
                .show(ui, |ui| {
                    for (seg_id, seg_text, seg_lemmas) in &segment_details {
                        let buf_key = format!("{}_{}_{}", tier_id, seg_id, cur_selected_idx);

                        ui.horizontal(|ui| {
                            // Segment ID badge
                            ui.label(
                                egui::RichText::new(format!("{}:", seg_id))
                                    .monospace()
                                    .strong()
                                    .color(egui::Color32::LIGHT_BLUE),
                            );

                            // Editable text for this segment
                            if let Some(buf) = state.seg_edit_buffers.get_mut(&buf_key) {
                                let response = ui.add(
                                    egui::TextEdit::singleline(buf)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::TextStyle::Body)
                                        .id_source(format!("seg_edit_{}", buf_key)),
                                );
                                // Commit on lost focus: emit edit seg command
                                if response.lost_focus() && *buf != *seg_text {
                                    seg_commands.push(format!(
                                        "edit seg {} {} {} {}",
                                        cur_selected_idx + 1, tier_id, seg_id, buf
                                    ));
                                }
                            }

                            // Delete segment button (only if more than 1)
                            if segment_details.len() > 1 {
                                if ui.small_button("✕").on_hover_text("Remove segment").clicked() {
                                    seg_commands.push(format!(
                                        "rm seg {} {} {}",
                                        cur_selected_idx + 1, tier_id, seg_id
                                    ));
                                }
                            }
                        });

                        // Lemma + rank display below the segment
                        if !seg_lemmas.is_empty() {
                            let lemma_display: Vec<String> = seg_lemmas.iter().map(|l| {
                                match frequency_manager::get_rank_for_lemma(l) {
                                    Some(r) => format!("{} <{}>", l, r),
                                    None => format!("{} <?>", l),
                                }
                            }).collect();
                            ui.label(
                                egui::RichText::new(format!("  {}", lemma_display.join(", ")))
                                    .small()
                                    .color(egui::Color32::GRAY),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new("  (no lemmas — run lemmatize)")
                                    .small()
                                    .italics()
                                    .color(egui::Color32::DARK_GRAY),
                            );
                        }

                        ui.add_space(4.0);
                    }

                    // Action buttons
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("+ Add Segment").clicked() {
                            let last_seg_id = segment_details.last()
                                .map(|(id, _, _)| id.as_str())
                                .unwrap_or("S0");
                            seg_commands.push(format!(
                                "add seg {} {} {} (new segment)",
                                cur_selected_idx + 1, tier_id, last_seg_id
                            ));
                        }
                        if ui.button("🔍 Lemmatize").on_hover_text("Re-lemmatize all segments via SpaCy").clicked() {
                            seg_commands.push(format!(
                                "lemmatize {} {}",
                                cur_selected_idx + 1, tier_id
                            ));
                        }
                        if ui.button("✓ Validate").on_hover_text("Lemmatize + mark tier Valid").clicked() {
                            seg_commands.push(format!(
                                "validate {} {}",
                                cur_selected_idx + 1, tier_id
                            ));
                        }
                    });
                });

            // Dispatch the first accumulated command (only one per frame)
            if let Some(cmd) = seg_commands.into_iter().next() {
                state.pending_terminal_command = Some(cmd);
                // Clear edit buffers so they resync with updated segment data next frame
                state.seg_edit_buffers.retain(|k, _| !k.starts_with(&format!("{}_{}", tier_id, "")));
            }
        } else if is_mapping_focused {
            // ── Mapping table view (Bas B / Bas T) ───────────────────────
            // Show the full sentence text (read-only) at top
            ui.label(
                egui::RichText::new(&current_tier_text)
                    .italics()
                    .color(egui::Color32::BLACK),
            );

            // "Generate Mapping" button below the sentence label
            if parent_tier_id.is_some() && prompt_name.is_some() {
                if ui.button("🧠 Generate Mapping").clicked() {
                    pending_action = Some("GenerateMapping");
                }
            }
            ui.add_space(4.0);

            let mut map_commands: Vec<String> = Vec::new();

            // Determine max word index for range validation
            let max_word_idx = mapping_rows.iter()
                .filter(|(is_w, ..)| *is_w)
                .map(|(_, idx, ..)| *idx)
                .max()
                .unwrap_or(0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(text_max_h)
                .show(ui, |ui| {
                    egui::Grid::new(format!("mapping_table_{}_{}", tier_id, cur_selected_idx))
                        .striped(true)
                        .min_col_width(30.0)
                        .show(ui, |ui| {
                            // Header row
                            ui.label(""); // checkbox column
                            ui.strong("#");
                            ui.strong("Source");
                            ui.strong("Target");
                            ui.strong("Lemmas");
                            ui.end_row();

                            for (is_word, word_idx, bg_ref, source_text, target, lemma_disp) in &mapping_rows {
                                if *is_word {
                                    // ── Word row ──
                                    // Checkbox for selection
                                    let mut selected = state.mapping_selected_rows.contains(word_idx);
                                    if ui.checkbox(&mut selected, "").changed() {
                                        if selected {
                                            state.mapping_selected_rows.insert(*word_idx);
                                        } else {
                                            state.mapping_selected_rows.remove(word_idx);
                                        }
                                    }

                                    // Word index
                                    ui.label(
                                        egui::RichText::new(format!("{}", word_idx))
                                            .monospace()
                                            .strong(),
                                    );

                                    // Editable source word text
                                    let src_key = format!("ms_{}_{}_{}", tier_id, cur_selected_idx, word_idx);
                                    if !state.seg_edit_buffers.contains_key(&src_key) {
                                        state.seg_edit_buffers.insert(src_key.clone(), source_text.clone());
                                    }
                                    if let Some(buf) = state.seg_edit_buffers.get_mut(&src_key) {
                                        let resp = ui.add(
                                            egui::TextEdit::singleline(buf)
                                                .desired_width(140.0)
                                                .font(egui::TextStyle::Body)
                                                .id_source(format!("ms_edit_{}", src_key)),
                                        );
                                        if resp.lost_focus() && *buf != *source_text {
                                            map_commands.push(format!("edit_word {} {}", word_idx, buf));
                                        }
                                    }

                                    // Editable target text
                                    let tgt_key = format!("mt_{}_{}_{}", tier_id, cur_selected_idx, word_idx);
                                    if !state.seg_edit_buffers.contains_key(&tgt_key) {
                                        state.seg_edit_buffers.insert(tgt_key.clone(), target.clone());
                                    }
                                    if let Some(buf) = state.seg_edit_buffers.get_mut(&tgt_key) {
                                        let resp = ui.add(
                                            egui::TextEdit::singleline(buf)
                                                .desired_width(140.0)
                                                .font(egui::TextStyle::Body)
                                                .id_source(format!("mt_edit_{}", tgt_key)),
                                        );
                                        if resp.lost_focus() && *buf != *target {
                                            map_commands.push(format!("edit_target {} {}", word_idx, buf));
                                        }
                                    }

                                    // Lemma display
                                    ui.label(
                                        egui::RichText::new(&**lemma_disp)
                                            .small()
                                            .color(egui::Color32::GRAY),
                                    );
                                } else {
                                    // ── Background / fill row ──
                                    ui.label(""); // no checkbox for bg rows
                                    ui.label(""); // no index

                                    // Editable background text
                                    let bg_key = format!("mb_{}_{}_{}", tier_id, cur_selected_idx, bg_ref);
                                    let display_text = if source_text.is_empty() { String::new() } else { source_text.clone() };
                                    if !state.seg_edit_buffers.contains_key(&bg_key) {
                                        state.seg_edit_buffers.insert(bg_key.clone(), display_text.clone());
                                    }
                                    if let Some(buf) = state.seg_edit_buffers.get_mut(&bg_key) {
                                        let resp = ui.add(
                                            egui::TextEdit::singleline(buf)
                                                .desired_width(140.0)
                                                .font(egui::TextStyle::Body)
                                                .text_color(egui::Color32::DARK_GRAY)
                                                .id_source(format!("mb_edit_{}", bg_key)),
                                        );
                                        if resp.lost_focus() && *buf != display_text {
                                            map_commands.push(format!("edit_b {} \"{}\"", bg_ref, buf));
                                        }
                                    }

                                    ui.label(""); // no target column
                                    ui.label(""); // no lemma column
                                }
                                ui.end_row();
                            }
                        });

                    // ── Row action toolbar ──
                    ui.separator();
                    ui.horizontal(|ui| {
                        let sel: Vec<usize> = state.mapping_selected_rows.iter().copied().collect();
                        let sel_count = sel.len();

                        // Merge: requires 2+ contiguous selected word rows
                        let can_merge = sel_count >= 2 && {
                            let min = sel[0];
                            let max = sel[sel_count - 1];
                            max - min + 1 == sel_count // contiguous check
                        };
                        if ui.add_enabled(can_merge, egui::Button::new("⛙ Merge"))
                            .on_hover_text("Merge selected contiguous words into one token")
                            .clicked()
                        {
                            map_commands.push(format!("merge {} {}", sel[0], sel[sel_count - 1]));
                            state.mapping_selected_rows.clear();
                        }

                        // Split: requires exactly 1 selected word
                        if ui.add_enabled(sel_count == 1, egui::Button::new("✂ Split"))
                            .on_hover_text("Split selected word into sub-tokens")
                            .clicked()
                        {
                            map_commands.push(format!("split {}", sel[0]));
                            state.mapping_selected_rows.clear();
                        }

                        // Insert: inserts before the first selected, or at end
                        let insert_at = if sel_count >= 1 { sel[0] } else { max_word_idx + 1 };
                        if ui.button("+ Insert")
                            .on_hover_text("Insert empty word token at selected position")
                            .clicked()
                        {
                            map_commands.push(format!("insert {}", insert_at));
                            state.mapping_selected_rows.clear();
                        }

                        // Delete: requires 1+ selected
                        if ui.add_enabled(sel_count >= 1, egui::Button::new("✕ Delete"))
                            .on_hover_text("Delete selected word(s)")
                            .clicked()
                        {
                            // Delete in reverse order to keep indices stable
                            for idx in sel.iter().rev() {
                                map_commands.push(format!("delete {}", idx));
                            }
                            state.mapping_selected_rows.clear();
                        }

                        ui.separator();

                        if ui.button("✓ Accept Map")
                            .on_hover_text("Validate the mapping")
                            .clicked()
                        {
                            map_commands.push("accept map".to_string());
                        }
                    });
                });

            // Only sync selected_tier_id when the user clicks this tab
            // (not on every frame), so that terminal `select tier` commands
            // are not overridden by the GUI rendering loop.
            // The tab is considered "clicked" when ui has focus and
            // the tier doesn't match — but we now rely on the tab-bar
            // selection code to set this, not the render body.

            // Dispatch first command
            if let Some(cmd) = map_commands.into_iter().next() {
                state.pending_terminal_command = Some(cmd);
                // Clear ALL mapping edit buffers so widget IDs resync with new data next frame
                state.seg_edit_buffers.clear();
                state.mapping_selected_rows.clear();
            }
        } else {
            // ── Standard full-text editor (Base) ──
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(text_max_h)
                .show(ui, |ui| {
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut current_tier_text)
                            .font(egui::TextStyle::Body)
                            .desired_width(f32::INFINITY)
                            .id_source(format!("editor_{tier_id}")),
                    );
                    if response.changed() {
                        pending_text_update = Some(current_tier_text.clone());
                    }
                });

            // ── Segment breakdown (non-segment-focused tiers) ────────────
            if segment_details.len() > 1 {
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("Segments ({})", segment_details.len()))
                        .small()
                        .strong(),
                );
                for (seg_id, seg_text, _) in &segment_details {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{seg_id}:"))
                                .monospace()
                                .color(egui::Color32::GRAY),
                        );
                        ui.label(seg_text.as_str());
                    });
                }
            }
        }
    } else {
        // Tier doesn't exist
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("Tier does not exist.").weak());
                if let (Some(p_text), Some(_)) = (&parent_text_opt, prompt_name) {
                    if !p_text.trim().is_empty()
                        && ui.button("🧠 Generate from Parent").clicked()
                    {
                        pending_action = Some("Regenerate");
                    }
                } else if parent_tier_id.is_none() && ui.button("Create Empty Tier").clicked() {
                    pending_text_update = Some(String::new());
                }
            });
        });
    }

    // ── 4. Apply deferred mutations ──────────────────────────────────────
    if let Some(new_text) = pending_text_update {
        if let Some(sentence_mut) = state.get_current_sentence_mut() {
            sentence_mut.update_tier_text(tier_id, new_text);
        }
    }

    if let Some("Retokenize") = pending_action {
        state.pending_terminal_command = Some(format!("approve edits {} {}", cur_selected_idx, tier_id));
        pending_action = None;
    }

    // Handle Approve — uses the bare approve command (sentinel resolution)
    if let Some("Approve") = pending_action {
        let one_based = cur_selected_idx + 1;
        state.pending_terminal_command = Some(format!("approve tier {} {}", one_based, tier_id));
        pending_action = None;
    }

    // 2. Handle Regenerate via command system
    //    For mapping-focused tiers (Bas B / Bas T), the GenerateStage engine
    //    handler automatically queues follow-up mapping generation.
    if let Some("Regenerate") = pending_action {
        if let Some(stage) = stage_for_tier(tier_id) {
            // run generate expects 1-based indices; cur_selected_idx is 0-based
            let one_based = cur_selected_idx + 1;
            state.pending_terminal_command = Some(format!(
                "run generate {} {} {}",
                stage, one_based, one_based
            ));
        }
    }

    // 3. Handle Generate Mapping (only mapping, no sentence regeneration)
    if let Some("GenerateMapping") = pending_action {
        let map_stage = if tier_id == "basic_base" {
            "GeneratePhraseMap"
        } else {
            "GenerateInversePhraseMap"
        };
        let one_based = cur_selected_idx + 1;
        state.pending_terminal_command = Some(format!(
            "run generate {} {} {}",
            map_stage, one_based, one_based
        ));
    }

    // ── LLM History panel ────────────────────────────────────────────────────
    // Shows every LLM call record for this sentence/tier, most-recent first.
    // Errors appear in red so they are immediately visible; the full generated
    // text for successful calls is in a monospace scroll area.
    if !llm_log.is_empty() {
        ui.separator();
        let success_count = llm_log.iter().filter(|r| r.is_success()).count();
        let error_count   = llm_log.len() - success_count;
        let header_label  = if error_count > 0 {
            format!("🕐 LLM History ({} ok, {} failed)", success_count, error_count)
        } else {
            format!("🕐 LLM History ({} calls)", llm_log.len())
        };

        egui::CollapsingHeader::new(header_label)
            .default_open(error_count > 0)   // auto-open when there are errors
            .id_source(format!("llm_history_{tier_id}_{cur_selected_idx}"))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_source(format!("llm_hist_scroll_{tier_id}_{cur_selected_idx}"))
                    .max_height(280.0)
                    .show(ui, |ui| {
                        // Most-recent call first.
                        for record in llm_log.iter().rev() {
                            let (icon, header_color) = if record.is_success() {
                                ("✅", egui::Color32::DARK_GREEN)
                            } else {
                                ("❌", egui::Color32::RED)
                            };
                            let applied_note = if !record.applied { " [not applied]" } else { "" };
                            ui.colored_label(
                                header_color,
                                format!(
                                    "{} {}  {}  ({}){applied_note}",
                                    icon, record.timestamp, record.stage, record.model
                                ),
                            );
                            if let Some(err) = &record.error {
                                ui.colored_label(
                                    egui::Color32::RED,
                                    egui::RichText::new(format!("  Error: {err}")).small(),
                                );
                            }
                            if let Some(text) = &record.generated_text {
                                // Show a compact scrollable preview of the generated text.
                                let preview = if text.len() > 600 {
                                    format!("{}…", &text[..600])
                                } else {
                                    text.clone()
                                };
                                egui::Frame::dark_canvas(ui.style()).show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&preview)
                                                .small()
                                                .monospace()
                                                .color(egui::Color32::LIGHT_GRAY),
                                        )
                                        .wrap(true),
                                    );
                                });
                            }
                            ui.add_space(4.0);
                        }
                    });
            });
    }
}
