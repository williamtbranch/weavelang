// src/gui/components/navigator.rs

use crate::gui::preview;
use crate::app::state::{AppState, TierView};
use eframe::egui;

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
                    state.pending_terminal_command = Some(format!("select sentence {}", idx));
                }
            });
        });

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
                            // Consider selected if within selected_range or equals selected_sentence_idx
                            let is_selected = match state.selected_range {
                                Some((s, e)) => i >= s && i <= e,
                                None => i == state.selected_sentence_idx,
                            };

                    let text = if state.left_view == TierView::Simulation {
                        let recipe = state.get_effective_recipe();
                        preview::generate_preview_text(sentence, &recipe)
                    } else {
                        let tier_id = match state.left_view {
                            TierView::Base => "base",
                            TierView::AdvancedTarget => "advanced_target",
                            TierView::ModerateTarget => "moderate_target",
                            TierView::BasicTarget => "basic_target",
                            TierView::BasicBase => "basic_base",
                            TierView::Simulation => unreachable!(),
                        };
                        sentence
                            .get_tier(tier_id)
                            .map(|t| t.full_text())
                            .unwrap_or_else(|| "---".to_string())
                    };

                    let label_text = format!("{}: {}", sentence.id, text);

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
                            // single select
                            state.pending_terminal_command = Some(format!("select sentence {}", i));
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
                                    let tier_id = match state.left_view {
                                        TierView::Base => "base",
                                        TierView::AdvancedTarget => "advanced_target",
                                        TierView::ModerateTarget => "moderate_target",
                                        TierView::BasicTarget => "basic_target",
                                        TierView::BasicBase => "basic_base",
                                        TierView::Simulation => unreachable!(),
                                    };
                                    let text = sent.get_tier(tier_id).map(|t| t.full_text()).unwrap_or_default();
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
                    let tier_id = match state.left_view {
                        TierView::Base => "base",
                        TierView::AdvancedTarget => "advanced_target",
                        TierView::ModerateTarget => "moderate_target",
                        TierView::BasicTarget => "basic_target",
                        TierView::BasicBase => "basic_base",
                        TierView::Simulation => unreachable!(),
                    };
                    let text = sent.get_tier(tier_id).map(|t| t.full_text()).unwrap_or_default();
                    parts.push(format!("{}: {}", sent.id, text));
                }
            }
            let joined = parts.join("\n\n");
            ui.ctx().output_mut(|o| o.copied_text = joined.clone());
        }

        // Handle Paste events to apply to primary selected sentence
        let events = ui.ctx().input(|i| i.events.clone());
        for ev in &events {
            if let egui::Event::Paste(pasted) = ev {
                let target_idx = state.selected_range.map(|(s, _)| s).unwrap_or(state.selected_sentence_idx);
                let tier_id = match state.left_view {
                    TierView::Base => "base",
                    TierView::AdvancedTarget => "advanced_target",
                    TierView::ModerateTarget => "moderate_target",
                    TierView::BasicTarget => "basic_target",
                    TierView::BasicBase => "basic_base",
                    TierView::Simulation => unreachable!(),
                };
                state.pending_terminal_command = Some(format!("update text {} {} {}", target_idx, tier_id, pasted));
                // clear range, make this the single selection
                state.selected_range = None;
                state.selected_sentence_idx = target_idx;
            }
        }
    });
}
