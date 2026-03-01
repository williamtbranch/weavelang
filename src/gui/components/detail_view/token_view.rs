// src/gui/components/detail_view/token_view.rs

use crate::domain::sentence::Sentence;
use crate::domain::token_stream::Token;
use crate::app::state::TierView;
use eframe::egui;

pub fn render(ui: &mut egui::Ui, sentence: &Sentence, view: TierView) {
    if let TierView::Simulation = view {
        ui.centered_and_justified(|ui| {
            ui.label("Token inspection is not available for Live Simulation.");
        });
        return;
    }

    let tier_id = match view {
        TierView::Base => "base",
        TierView::AdvancedTarget => "advanced_target",
        TierView::ModerateTarget => "moderate_target",
        TierView::BasicTarget => "basic_target",
        TierView::BasicBase => "basic_base",
        TierView::Simulation => unreachable!(),
    };

    ui.heading(format!("Token Inspector: {tier_id}"));
    ui.separator();

    if let Some(tier) = sentence.get_tier(tier_id) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 5.0);

                    // FIX: Iterate over segments
                    for (i, segment) in tier.segments.iter().enumerate() {
                        // Visual divider between segments
                        if i > 0 {
                            ui.add(egui::Label::new(
                                egui::RichText::new(" | ")
                                    .color(egui::Color32::LIGHT_GRAY)
                                    .strong(),
                            ));
                        }

                        for token in segment.stream.tokens() {
                            match token {
                                Token::Background(text) => {
                                    ui.add(egui::Label::new(egui::RichText::new(text).monospace()));
                                }
                                Token::Word(word_data) => {
                                    let text = egui::RichText::new(format!(" {} ", word_data.text))
                                        .color(egui::Color32::BLACK)
                                        .background_color(egui::Color32::from_rgb(200, 255, 200));

                                    let response = ui
                                        .add(egui::Label::new(text).sense(egui::Sense::click()))
                                        .on_hover_ui(|ui| {
                                            ui.strong("Word Data");
                                            ui.label(format!("ID: {:?}", word_data.id));
                                            ui.label(format!("Lemmas: {:?}", word_data.lemmas));
                                        });

                                    if response.clicked() {
                                        println!("Clicked word: {}", word_data.text);
                                    }
                                }
                            }
                        }
                    }
                });
            });
    } else {
        ui.colored_label(
            egui::Color32::RED,
            format!("Tier '{tier_id}' data not found."),
        );
    }
}
