// src/gui/components/detail_view/text_view.rs

use crate::domain::tier::TierState;
use crate::domain::llm_log::LlmCallRecord;
use crate::gui::preview;
use crate::app::state::{AppState, TierView};
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
    let segment_details: Vec<(String, String)> = state
        .get_current_sentence()
        .and_then(|s| s.get_tier(tier_id))
        .map(|t| {
            t.segments
                .iter()
                .map(|seg| (seg.id.clone(), seg.full_text()))
                .collect()
        })
        .unwrap_or_default();

    if let Some(sentence_mut) = state.get_current_sentence_mut() {
        let _s_id = sentence_mut.id.clone();
        if let Some(tier) = sentence_mut.get_tier(tier_id) {
            current_tier_text = tier.full_text();
            current_tier_state = tier.state;
            current_tier_exists = true;
        }

        if current_tier_exists {
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
                        if ui.button("🧠 Regenerate via LLM").clicked() {
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
                     if ui.button("🧠 Regenerate (Discard Edits)").clicked() {
                        pending_action = Some("Regenerate");
                     }
                }
            });

            if current_tier_state != TierState::Valid {
                ui.separator();
            }

            // Leave vertical room for the LLM history panel.
            let text_max_h = if llm_log.is_empty() { f32::INFINITY } else { 240.0 };
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

            // ── Segment breakdown ────────────────────────────────────────────
            if segment_details.len() > 1 {
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("Segments ({})", segment_details.len()))
                        .small()
                        .strong(),
                );
                for (seg_id, seg_text) in &segment_details {
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
        } else {
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

        // Apply local text updates immediately (re-tokenizing is deferred if needed)
        if let Some(new_text) = pending_text_update {
            sentence_mut.update_tier_text(tier_id, new_text);
        }
        
        // Handle "Retokenize" immediately
        if let Some("Retokenize") = pending_action {
             state.pending_terminal_command = Some(format!("approve edits {} {}", cur_selected_idx, tier_id));
             pending_action = None;
        }
    } // End of mutable borrow

    // 2. Handle Regenerate via command system
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
