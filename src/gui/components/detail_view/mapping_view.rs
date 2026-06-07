// src/gui/components/detail_view/mapping_view.rs

use crate::app::state::{AppState, DetailView};
use eframe::egui;
use std::collections::HashMap;

// Signature changed: No &Sentence argument. state is passed directly.
pub fn render(ui: &mut egui::Ui, mode: DetailView, state: &mut AppState) {
    // 1. Determine Context
    let (source_tier_id, target_tier_id, _prompt_name) = match mode {
        DetailView::MappingDiglot => ("basic_base", "basic_target", "basic_diglot"),
        DetailView::MappingInverse => ("basic_target", "basic_base", "inverse_diglot"),
        _ => return,
    };

    ui.horizontal(|ui| {
        ui.heading(format!("Mapping: {source_tier_id} -> {target_tier_id}"));
    });
    ui.separator();

    // 2. Capture index before mutable borrow
    let cur_selected_idx = state.selected_sentence_idx;
    let mut pending_generate = false;

    // 3. Mutable Interaction (Generate Button)
    let mut pending_regenerate_sentence = false;

    if let Some(sentence_mut) = state.get_current_sentence_mut() {
        let _s_id = sentence_mut.id.clone();
        let source_text = sentence_mut
            .get_tier(source_tier_id)
            .map(|t| t.full_text())
            .unwrap_or_default();
        
        // Only source text is required to generate a mapping.
        let tiers_exist = !source_text.is_empty();

        if tiers_exist {
            ui.horizontal(|ui| {
                if ui
                    .button("🧠 Generate Mapping")
                    .on_hover_text("Sends source tier text to LLM for a new mapping")
                    .clicked()
                {
                    pending_generate = true;
                }
                if ui
                    .button("🔄 Regenerate Sentence")
                    .on_hover_text("Regenerates the source tier sentence text from upstream, then requires a new mapping")
                    .clicked()
                {
                    pending_regenerate_sentence = true;
                }
            });
        } else {
            ui.colored_label(egui::Color32::RED, "Source Tier missing.");
        }
    } // End mutable borrow

    // 4. Emit generate command via command system
    if pending_generate {
        let stage_name = match mode {
            DetailView::MappingDiglot  => "GeneratePhraseMap",
            DetailView::MappingInverse => "GenerateInversePhraseMap",
            _                          => return,
        };
        state.pending_terminal_command = Some(format!(
            "run generate {} {} {}",
            stage_name, cur_selected_idx, cur_selected_idx
        ));
    }

    if pending_regenerate_sentence {
        let stage_name = match mode {
            DetailView::MappingDiglot  => "GenerateBasicBase",
            DetailView::MappingInverse => "GenerateBasicTarget",
            _                          => return,
        };
        state.pending_terminal_command = Some(format!(
            "run generate {} {} {}",
            stage_name, cur_selected_idx, cur_selected_idx
        ));
    }

    ui.separator();

    // 4. Render Existing Mapping Table (Read-Only)
    // We re-acquire the sentence (immutably) for rendering the grid
    if let Some(sentence) = state.get_current_sentence() {
        let source_tier = match sentence.get_tier(source_tier_id) {
            Some(t) => t,
            None => {
                ui.label("Source Tier not found.");
                return;
            }
        };

        let mapping_lookup: HashMap<_, _> = sentence
            .mappings()
            .iter()
            .find(|m| m.from_tier_id == source_tier_id && m.to_tier_id == target_tier_id)
            .map(|tier_mapping| {
                tier_mapping

                    .entries
                    .iter()
                    .map(|entry| (entry.source_word_id, entry))
                    .collect()
            })
            .unwrap_or_default();

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("mapping_grid")
                .striped(true)
                .min_col_width(100.0)
                .show(ui, |ui| {
                    ui.strong("Source Token");
                    ui.strong("Mapped Target");
                    ui.end_row();

                    for segment in &source_tier.segments {
                        for token in segment.stream.tokens() {
                            if let crate::domain::token_stream::Token::Word(word_data) = token {
                                ui.label(&word_data.text);
                                if let Some(entry) = mapping_lookup.get(&word_data.id) {
                                    let color = if entry.is_viable {
                                        egui::Color32::DARK_GREEN
                                    } else {
                                        egui::Color32::GRAY
                                    };
                                    ui.colored_label(color, &entry.target_text);
                                } else {
                                    ui.weak("---");
                                }
                                ui.end_row();
                            }
                        }
                    }
                });
        });
    }
}
