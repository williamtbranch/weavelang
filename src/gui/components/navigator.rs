// src/gui/components/navigator.rs

use eframe::egui;
use crate::gui::state::{AppState, TierView};
use crate::gui::preview;
use crate::domain::sentence::Sentence;
use crate::domain::tier::Tier;
use crate::domain::segment::Segment;
use crate::domain::token_stream::TokenStream;

pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    ui.vertical(|ui| {
        // --- Header with Add Button ---
        ui.horizontal(|ui| {
            ui.heading("Navigator");
            
            // NEW: Add Sentence Button
            if ui.button("➕").on_hover_text("Add new empty sentence").clicked() {
                let new_id = format!("S{}", state.document.len() + 1);
                let mut sentence = Sentence::new(new_id);
                
                // Pre-populate the 'base' tier
                let mut tier = Tier::new("base".to_string());
                tier.add_segment(Segment::from_stream(
                    "S1".to_string(),
                    TokenStream::new(""), 
                    vec![]
                ));
                sentence.add_tier(tier);
                
                state.document.push(sentence);
                // Select the new sentence
                state.selected_sentence_idx = state.document.len() - 1;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut current_num = state.selected_sentence_idx + 1;
                let total = state.document.len();
                ui.label(format!("/ {}", total));
                let drag = ui.add(egui::DragValue::new(&mut current_num).speed(1.0).clamp_range(1..=total));
                if drag.changed() && total > 0 {
                    let idx = (current_num - 1).clamp(0, total - 1);
                    state.selected_sentence_idx = idx;
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
                    let is_selected = i == state.selected_sentence_idx;

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
                        sentence.get_tier(tier_id)
                            .map(|t| t.full_text())
                            .unwrap_or_else(|| "---".to_string())
                    };

                    let label_text = format!("{}: {}", sentence.id, text);
                    
                    if ui.add(egui::SelectableLabel::new(is_selected, label_text)).clicked() {
                        state.selected_sentence_idx = i;
                    }
                }
            });
    });
}