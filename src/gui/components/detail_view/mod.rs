use crate::app::state::{AppState, DetailView, TierView};
use eframe::egui;

pub mod mapping_view;
pub mod proper_noun_view;
pub mod text_view;
pub mod token_view;

pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    ui.vertical(|ui| {
        // --- Tab Bar ---
        ui.horizontal(|ui| {
            ui.label("View:");

            selectable_tier(ui, state, "Source", TierView::Base);
            ui.separator();
            selectable_tier(ui, state, "Adv", TierView::AdvancedTarget);
            selectable_tier(ui, state, "Mod", TierView::ModerateTarget);
            selectable_tier(ui, state, "Bas T", TierView::BasicTarget);
            selectable_tier(ui, state, "Bas B", TierView::BasicBase);

            ui.separator();

            // Proper Noun Lemmas tab
            if ui
                .selectable_label(
                    matches!(state.right_view, DetailView::ProperNounLemmas),
                    "PN Lemmas",
                )
                .clicked()
            {
                state.pending_terminal_command = Some("set right_view proper_noun_lemmas".to_string());
            }
        });

        ui.separator();

        // --- Content Area ---
        match state.right_view {
            DetailView::Tier(tier_view) => {
                text_view::render(ui, tier_view, state);
            }
            // Legacy views: fall through to tier view for the relevant tier
            DetailView::Token(tier_view) => {
                text_view::render(ui, tier_view, state);
            }
            DetailView::MappingDiglot => {
                text_view::render(ui, TierView::BasicBase, state);
            }
            DetailView::MappingInverse => {
                text_view::render(ui, TierView::BasicTarget, state);
            }
            DetailView::ProperNounLemmas => {
                proper_noun_view::render(ui, state);
            }
        }
    });
}

/// Helper to draw a tier selection tab.
fn selectable_tier(ui: &mut egui::Ui, state: &mut AppState, label: &str, view: TierView) {
    let is_active_context = match state.right_view {
        DetailView::Tier(v) | DetailView::Token(v) => v == view,
        _ => false,
    };

    if ui.selectable_label(is_active_context, label).clicked() {
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
