// src/gui/components/info_bar.rs

use eframe::egui;
use crate::gui::state::{AppState, SimulationMode};

pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        ui.strong("Simulation:");
        ui.separator();

        ui.radio_value(&mut state.sim_mode, SimulationMode::Calibrated, "Book Level");
        ui.radio_value(&mut state.sim_mode, SimulationMode::Manual, "Manual Recipe");
        
        ui.separator();

        match state.sim_mode {
            SimulationMode::Calibrated => {
                ui.label("User Level:");
                ui.add(
                    egui::DragValue::new(&mut state.sim_user_level)
                        .speed(1.0)
                        .clamp_range(1..=50)
                );
                if state.book_map.is_none() {
                    ui.colored_label(egui::Color32::RED, "(No Map)");
                }
            },
            SimulationMode::Manual => {
                ui.label("Bas:");
                ui.add(egui::DragValue::new(&mut state.sim_manual_recipe.bas).speed(10));
                
                ui.label("Mod:");
                ui.add(egui::DragValue::new(&mut state.sim_manual_recipe.mod_v).speed(10));
                
                ui.label("Adv:");
                ui.add(egui::DragValue::new(&mut state.sim_manual_recipe.adv).speed(10));
            }
        }

        ui.separator();

        if let Some(sentence) = state.get_current_sentence() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("ID: {}", sentence.id));
            });
        }
    });
}