use crate::domain::tier::TierState;
use crate::app::state::{AppState, DetailView, TierView};
use eframe::egui; // Import to check state

pub mod mapping_view;
pub mod text_view;
pub mod token_view;

pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    ui.vertical(|ui| {
        // --- Tab Bar ---
        ui.horizontal(|ui| {
            ui.label("View:");

            // Tier Tabs - Clicking these now ALWAYS goes to Text View
            selectable_tier(ui, state, "Adv", TierView::AdvancedTarget);
            selectable_tier(ui, state, "Mod", TierView::ModerateTarget);
            selectable_tier(ui, state, "Bas T", TierView::BasicTarget);
            selectable_tier(ui, state, "Bas B", TierView::BasicBase);

            ui.separator();

            // --- Tokens Button with Toggle & Dirty Logic ---

            // 1. Determine current tier context
            let current_tier_view = match state.right_view {
                DetailView::Tier(t) => Some(t),
                DetailView::Token(t) => Some(t),
                _ => None, // Mappings don't map 1:1 to a single tier for this button
            };

            // 2. Check Dirty State
            let mut button_color = egui::Color32::default(); // Default text color
            if let Some(tier_view) = current_tier_view {
                if let Some(sentence) = state.get_current_sentence() {
                    let tier_id = match tier_view {
                        TierView::Base => "base",
                        TierView::AdvancedTarget => "advanced_target",
                        TierView::ModerateTarget => "moderate_target",
                        TierView::BasicTarget => "basic_target",
                        TierView::BasicBase => "basic_base",
                        TierView::Simulation => "simulation", // Dummy
                    };

                    if let Some(tier) = sentence.get_tier(tier_id) {
                        if tier.state == TierState::Dirty {
                            // Make button orange if tokens are invalid
                            button_color = egui::Color32::from_rgb(255, 140, 0);
                        }
                    }
                }
            }

            let is_token_mode = matches!(state.right_view, DetailView::Token(_));

            // 3. Render Button
            let tokens_btn =
                egui::Button::new(egui::RichText::new("Tokens").color(if is_token_mode {
                    egui::Color32::WHITE
                } else {
                    button_color
                }))
                .selected(is_token_mode);

            if ui.add(tokens_btn).clicked() {
                if is_token_mode {
                    // TOGGLE OFF: Go back to Text view for the current tier
                    if let DetailView::Token(t) = state.right_view {
                        let view_str = match t {
                            TierView::Base => "base",
                            TierView::AdvancedTarget => "advanced_target",
                            TierView::ModerateTarget => "moderate_target",
                            TierView::BasicTarget => "basic_target",
                            TierView::BasicBase => "basic_base",
                            TierView::Simulation => "simulation",
                        };
                        state.pending_terminal_command = Some(format!("set right_view {}", view_str));
                    }
                } else {
                    // TOGGLE ON: Switch to Token view
                    // If we are in a Tier view, use that tier. Otherwise default to BasicBase.
                    let target_tier = match state.right_view {
                        DetailView::Tier(t) => t,
                        _ => TierView::BasicBase,
                    };
                    let view_str = match target_tier {
                        TierView::Base => "token_base",
                        TierView::AdvancedTarget => "token_advanced_target",
                        TierView::ModerateTarget => "token_moderate_target",
                        TierView::BasicTarget => "token_basic_target",
                        TierView::BasicBase => "token_basic_base",
                        TierView::Simulation => "token_simulation",
                    };
                    state.pending_terminal_command = Some(format!("set right_view {}", view_str));
                }
            }

            ui.separator();

            // Mapping Views
            if ui
                .selectable_label(
                    matches!(state.right_view, DetailView::MappingDiglot),
                    "Map (Fwd)",
                )
                .clicked()
            {
                state.pending_terminal_command = Some("set right_view mapping_diglot".to_string());
            }

            if ui
                .selectable_label(
                    matches!(state.right_view, DetailView::MappingInverse),
                    "Map (Inv)",
                )
                .clicked()
            {
                state.pending_terminal_command = Some("set right_view mapping_inverse".to_string());
            }
        });

        ui.separator();

        // --- Content Area ---
        match state.right_view {
            DetailView::Tier(tier_view) => {
                text_view::render(ui, tier_view, state);
            }
            DetailView::Token(tier_view) => {
                if let Some(sentence) = state.get_current_sentence() {
                    token_view::render(ui, sentence, tier_view);
                } else {
                    ui.label("No sentence selected.");
                }
            }
            DetailView::MappingDiglot | DetailView::MappingInverse => {
                mapping_view::render(ui, state.right_view, state);
            }
        }
    });
}

/// Helper to draw a tier selection tab.
fn selectable_tier(ui: &mut egui::Ui, state: &mut AppState, label: &str, view: TierView) {
    // Highlight if we are in Tier Mode OR Token Mode for this specific tier
    let is_active_context = match state.right_view {
        DetailView::Tier(v) => v == view,
        DetailView::Token(v) => v == view,
        _ => false,
    };

    // We strictly check for Tier mode to determine "Selected" visual style for the tab itself?
    // Usually tabs highlight if their content is visible.
    // Let's highlight it if it's the active context.

    if ui.selectable_label(is_active_context, label).clicked() {
        // ACTION: Always switch to Text View (DetailView::Tier)
        // This solves "I can't get back to edit".
        let view_str = match view {
            TierView::Base => "base",
            TierView::AdvancedTarget => "advanced_target",
            TierView::ModerateTarget => "moderate_target",
            TierView::BasicTarget => "basic_target",
            TierView::BasicBase => "basic_base",
            TierView::Simulation => "simulation",
        };
        state.pending_terminal_command = Some(format!("set right_view {}", view_str));
    }
}
