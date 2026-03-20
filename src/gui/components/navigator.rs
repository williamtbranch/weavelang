// src/gui/components/navigator.rs

use crate::gui::preview;
use crate::app::state::{AppState, TierView};
use crate::domain::sentence::Completeness;
use eframe::egui;

/// Colours used for sentence completeness backgrounds.
const COLOR_COMPLETE: egui::Color32   = egui::Color32::from_rgb(200, 240, 200); // light green
const COLOR_INCOMPLETE: egui::Color32 = egui::Color32::from_rgb(255, 248, 210); // pale yellow
const COLOR_EMPTY: egui::Color32      = egui::Color32::from_rgb(255, 255, 255); // white

/// Status bar colours.
const STATUS_READY: egui::Color32      = egui::Color32::from_rgb(180, 230, 180);
const STATUS_INCOMPLETE: egui::Color32 = egui::Color32::from_rgb(255, 240, 180);
const STATUS_EMPTY: egui::Color32      = egui::Color32::from_rgb(240, 240, 240);

/// Resolve the tier_id for the current navigator view.
fn tier_id_for_view(view: TierView) -> &'static str {
    match view {
        TierView::Base => "base",
        TierView::AdvancedTarget => "advanced_target",
        TierView::ModerateTarget => "moderate_target",
        TierView::BasicTarget => "basic_target",
        TierView::BasicBase => "basic_base",
        TierView::Simulation => "base", // simulation doesn't map to a single tier
    }
}

/// Compute the completeness of a sentence for the current navigator view.
///
/// - **Source (Base)** view is the "master": green means the entire sentence
///   is weave-ready (all tiers + mappings).
/// - Other views: green means that specific tier is valid/non-empty.
fn sentence_view_completeness(state: &AppState, idx: usize) -> Completeness {
    let sent = &state.document[idx];
    if state.left_view == TierView::Base || state.left_view == TierView::Simulation {
        // Master view — full weave completeness
        sent.weave_completeness()
    } else {
        sent.tier_completeness(tier_id_for_view(state.left_view))
    }
}

pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    ui.vertical(|ui| {
        // --- Header with Add Button ---
        ui.horizontal(|ui| {
            ui.heading("Navigator");

            // NEW: Add Sentence Button
            if ui
                .button("➕")
                .on_hover_text("Add new empty sentence")
                .clicked()
            {
                state.pending_terminal_command = Some("add sentence".to_string());
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut current_num = state.selected_sentence_idx + 1;
                let total = state.document.len();
                ui.label(format!("/ {total}"));
                let drag = ui.add(
                    egui::DragValue::new(&mut current_num)
                        .speed(1.0)
                        .clamp_range(1..=total),
                );
                if drag.changed() && total > 0 {
                    let idx = (current_num - 1).clamp(0, total - 1);
                    // select sentence uses 1-based numbers from user
                    state.pending_terminal_command = Some(format!("select sentence {}", idx + 1));
                }
            });
        });

        // --- Status Indicator Bar ---
        if !state.document.is_empty() {
            let total = state.document.len();
            let mut n_complete = 0usize;
            let mut n_incomplete = 0usize;
            for i in 0..total {
                match sentence_view_completeness(state, i) {
                    Completeness::Complete => n_complete += 1,
                    Completeness::Incomplete => n_incomplete += 1,
                    Completeness::Empty => {}
                }
            }

            let has_level_map = state.book_map.as_ref().map_or(false, |m| !m.is_empty());

            let (status_text, status_color) = if n_complete == total && has_level_map {
                ("Ready".to_string(), STATUS_READY)
            } else if n_complete == total && !has_level_map {
                ("Needs Calibration".to_string(), STATUS_INCOMPLETE)
            } else if n_complete + n_incomplete > 0 {
                (format!("Incomplete ({}/{})", n_complete, total), STATUS_INCOMPLETE)
            } else {
                ("Empty".to_string(), STATUS_EMPTY)
            };

            let frame = egui::Frame::none()
                .fill(status_color)
                .inner_margin(egui::Margin::symmetric(6.0, 3.0))
                .rounding(egui::Rounding::same(3.0));
            frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Weave Status:");
                    ui.label(&status_text);
                });
            });
        }

        ui.separator();

        if state.document.is_empty() {
            ui.label("No document loaded.");
            return;
        }

        let text_height = ui.text_style_height(&egui::TextStyle::Body);
        let row_height = text_height * 2.5;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_height, state.document.len(), |ui, row_range| {
                for i in row_range {
                    let sentence = &state.document[i];
                    let is_selected = match state.selected_range {
                        Some((s, e)) => i >= s && i <= e,
                        None => i == state.selected_sentence_idx,
                    };

                    // Determine completeness-based background color
                    let completeness = sentence_view_completeness(state, i);
                    let bg_color = match completeness {
                        Completeness::Complete => COLOR_COMPLETE,
                        Completeness::Incomplete => COLOR_INCOMPLETE,
                        Completeness::Empty => COLOR_EMPTY,
                    };

                    let text = if state.left_view == TierView::Simulation {
                        let recipe = state.get_effective_recipe();
                        preview::generate_preview_text(sentence, &recipe)
                    } else {
                        let tid = tier_id_for_view(state.left_view);
                        sentence
                            .get_tier(tid)
                            .map(|t| t.full_text())
                            .unwrap_or_else(|| "---".to_string())
                    };

                    let label_text = format!("{}: {}", sentence.id, text);

                    // Draw a colored background rect behind the selectable label
                    let row_rect = ui.available_rect_before_wrap();
                    let row_rect = egui::Rect::from_min_size(
                        row_rect.min,
                        egui::vec2(ui.available_width(), row_height),
                    );
                    if !is_selected {
                        ui.painter().rect_filled(row_rect, 0.0, bg_color);
                    }

                    let resp = ui.add(egui::SelectableLabel::new(is_selected, label_text));
                    // Handle click with modifiers for range selection
                    if resp.clicked() {
                        let mods = ui.ctx().input(|i| i.modifiers);
                        if mods.shift || mods.ctrl {
                            // create/extend a contiguous range from current primary selection to clicked
                            let cur = state.selected_sentence_idx;
                            let (s, e) = if cur <= i { (cur, i) } else { (i, cur) };
                            state.selected_range = Some((s, e));
                            state.selected_sentence_idx = s; // primary is first
                        } else {
                            // single select (1-based for terminal)
                            state.pending_terminal_command = Some(format!("select sentence {}", i + 1));
                        }
                    }

                    // Context menu for Copy / Copy ID
                    resp.context_menu(|ui| {
                        if ui.button("Copy").clicked() {
                            // collect selected texts
                            let (sidx, eidx) = match state.selected_range {
                                Some((s, e)) => (s, e),
                                None => (state.selected_sentence_idx, state.selected_sentence_idx),
                            };
                            let mut parts: Vec<String> = Vec::new();
                            for j in sidx..=eidx {
                                if let Some(sent) = state.document.get(j) {
                                    let tid = tier_id_for_view(state.left_view);
                                    let text = sent.get_tier(tid).map(|t| t.full_text()).unwrap_or_default();
                                    parts.push(format!("{}: {}", sent.id, text));
                                }
                            }
                            let joined = parts.join("\n\n");
                            ui.ctx().output_mut(|o| o.copied_text = joined.clone());
                            ui.close_menu();
                        }
                        if ui.button("Copy IDs").clicked() {
                            let (sidx, eidx) = match state.selected_range {
                                Some((s, e)) => (s, e),
                                None => (state.selected_sentence_idx, state.selected_sentence_idx),
                            };
                            let ids: Vec<String> = (sidx..=eidx)
                                .filter_map(|j| state.document.get(j).map(|s| s.id.clone()))
                                .collect();
                            ui.ctx().output_mut(|o| o.copied_text = ids.join("\n"));
                            ui.close_menu();
                        }
                    });
                }
            });

        // Keyboard shortcuts: Copy (Ctrl-C) and Paste (handled via Paste event)
        let copy_pressed = ui.ctx().input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::C));
        if copy_pressed {
            let (sidx, eidx) = match state.selected_range {
                Some((s, e)) => (s, e),
                None => (state.selected_sentence_idx, state.selected_sentence_idx),
            };
            let mut parts: Vec<String> = Vec::new();
            for j in sidx..=eidx {
                if let Some(sent) = state.document.get(j) {
                    let tid = tier_id_for_view(state.left_view);
                    let text = sent.get_tier(tid).map(|t| t.full_text()).unwrap_or_default();
                    parts.push(format!("{}: {}", sent.id, text));
                }
            }
            let joined = parts.join("\n\n");
            ui.ctx().output_mut(|o| o.copied_text = joined.clone());
        }

        // Handle Paste events to apply to primary selected sentence,
        // but ONLY when no text-editing widget (TextEdit) has keyboard focus.
        // If a TextEdit is focused the paste belongs to that widget, not here.
        let any_widget_focused = ui.ctx().memory(|m| m.focused().is_some());
        if !any_widget_focused {
            let events = ui.ctx().input(|i| i.events.clone());
            for ev in &events {
                if let egui::Event::Paste(pasted) = ev {
                    let target_idx = state.selected_range.map(|(s, _)| s).unwrap_or(state.selected_sentence_idx);
                    let tid = tier_id_for_view(state.left_view);
                    // update text uses 1-based sentence number
                    state.pending_terminal_command = Some(format!("update text {} {} {}", target_idx + 1, tid, pasted));
                    // clear range, make this the single selection
                    state.selected_range = None;
                    state.selected_sentence_idx = target_idx;
                }
            }
        }
    });
}
