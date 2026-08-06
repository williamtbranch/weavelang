// src/gui/components/raw_source_view.rs
//
// Raw Source tab: the ESCore-style adaptation workspace.
//
// Left column  — unit list with per-unit DRC status.
// Right column — the raw text, the current adapted draft, and the DRC report
//                (metrics + worst-offender table) for the selected unit.
//
// All mutations go through `pending_terminal_command`, so every action here
// has an exact terminal equivalent (`adapt draft`, `adapt squeeze`, ...).

use crate::app::state::AppState;
use crate::domain::raw_source::AdaptStatus;
use eframe::egui;

const PASS_COLOR: egui::Color32 = egui::Color32::from_rgb(80, 180, 80);
const FAIL_COLOR: egui::Color32 = egui::Color32::from_rgb(200, 140, 60);
const FLOOR_COLOR: egui::Color32 = egui::Color32::from_rgb(200, 100, 100);

fn status_color(status: AdaptStatus) -> egui::Color32 {
    match status {
        AdaptStatus::Passing => PASS_COLOR,
        AdaptStatus::Floor => FLOOR_COLOR,
        AdaptStatus::Drafted => FAIL_COLOR,
        AdaptStatus::NotStarted => egui::Color32::GRAY,
    }
}

pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    if state.raw_source.is_none() {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.25);
            ui.heading("No Raw Source");
            ui.add_space(8.0);
            ui.label("Use File › Import Raw... to load the original text.");
            ui.add_space(4.0);
            ui.label("It is adapted to the target level here, then promoted to the Source tab.");
        });
        return;
    }

    render_job_bar(ui, state);
    render_toolbar(ui, state);
    ui.separator();

    egui::SidePanel::left("raw_source_units")
        .resizable(true)
        .default_width(260.0)
        .show_inside(ui, |ui| render_unit_list(ui, state));

    render_unit_detail(ui, state);
}

/// Progress strip for a running adapt job.
fn render_job_bar(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(job) = state.adapt_job.clone() else {
        return;
    };
    let Ok(j) = job.lock() else {
        return;
    };

    ui.horizontal(|ui| {
        ui.spinner();
        ui.strong(&j.label);
        ui.label(format!("{}/{} unit(s)", j.done_units, j.total_units));
        if j.cancel_requested {
            ui.colored_label(FLOOR_COLOR, "cancelling…");
        } else if ui.button("Cancel").clicked() {
            state.pending_terminal_command = Some("adapt cancel".to_string());
        }
    });
    if let Some(last) = j.log.last() {
        ui.label(egui::RichText::new(last).weak().italics());
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
    ui.separator();
}

fn render_toolbar(ui: &mut egui::Ui, state: &mut AppState) {
    let running = state.adapt_job.is_some();
    let (name, unit_count, chapter_count, chunk, gates, domain_count, passing) = {
        let raw = state.raw_source.as_ref().unwrap();
        (
            raw.name.clone(),
            raw.units.len(),
            raw.chapter_groups().len(),
            raw.max_sentences_per_unit,
            raw.target,
            raw.domain_lemmas.len(),
            raw.units
                .iter()
                .filter(|u| u.status == AdaptStatus::Passing)
                .count(),
        )
    };

    ui.horizontal_wrapped(|ui| {
        ui.heading(&name);
        ui.label(format!(
            "{} chapter(s) · {} unit(s) · ≤ {} sentence(s)/unit",
            chapter_count, unit_count, chunk
        ))
        .on_hover_text(
            "Long chapters are split into parts so each LLM call stays manageable.\n\
             Parts are merged back into one chapter on promotion.\n\
             Change with: adapt set chunk <N> (before drafting).",
        );
        ui.colored_label(PASS_COLOR, format!("{} passing", passing));
        ui.separator();
        ui.label(format!(
            "target: iLevel ≤ {:.1} @ {:.0}% coverage · length {:.0}–{:.0}% · {} domain lemma(s)",
            gates.i_level_max,
            gates.coverage * 100.0,
            gates.min_percent,
            gates.max_percent,
            domain_count
        ));
    });

    ui.horizontal_wrapped(|ui| {
        let selected = current_selector(state);

        if ui
            .add_enabled(!running, egui::Button::new("▶ Run All"))
            .on_hover_text("Draft, then squeeze every unit until it passes or hits the floor")
            .clicked()
        {
            state.pending_terminal_command = Some("adapt run all".to_string());
        }
        if ui
            .add_enabled(!running, egui::Button::new("▶ Run Unit"))
            .clicked()
        {
            state.pending_terminal_command = Some(format!("adapt run {}", selected));
        }
        if ui
            .add_enabled(!running, egui::Button::new("Draft"))
            .on_hover_text("One draft pass — replaces the current draft")
            .clicked()
        {
            state.pending_terminal_command = Some(format!("adapt draft {}", selected));
        }
        if ui
            .add_enabled(!running, egui::Button::new("Squeeze"))
            .on_hover_text("One squeeze pass against the offender table")
            .clicked()
        {
            state.pending_terminal_command = Some(format!("adapt squeeze {}", selected));
        }
        if ui
            .add_enabled(!running, egui::Button::new("Score"))
            .on_hover_text("Re-run the DRC locally — no LLM call")
            .clicked()
        {
            state.pending_terminal_command = Some("adapt score all".to_string());
        }
        if ui
            .add_enabled(!running, egui::Button::new("Revert"))
            .on_hover_text("Roll this unit's draft back one version")
            .clicked()
        {
            state.pending_terminal_command = Some(format!("adapt revert {}", selected));
        }

        ui.separator();

        if ui
            .add_enabled(!running, egui::Button::new("Domain Vocab..."))
            .on_hover_text("Load the approved book-level vocabulary policy")
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Text File", &["txt"])
                .pick_file()
            {
                state.pending_terminal_command =
                    Some(format!("adapt domain {}", path.to_string_lossy()));
            }
        }

        ui.separator();

        let can_promote = passing > 0;
        if ui
            .add_enabled(!running && can_promote, egui::Button::new("⬆ Promote to Source"))
            .on_hover_text("Generate the source text from passing drafts and import it")
            .clicked()
        {
            state.pending_terminal_command = Some("adapt promote".to_string());
        }
        if ui
            .add_enabled(!running, egui::Button::new("⬆ Promote (force)"))
            .on_hover_text("Promote every drafted unit, including ones that never passed")
            .clicked()
        {
            state.pending_terminal_command = Some("adapt promote --force".to_string());
        }
    });
}

/// 1-based selector for the currently highlighted unit.
fn current_selector(state: &AppState) -> usize {
    state
        .raw_source
        .as_ref()
        .map(|r| r.selected_unit + 1)
        .unwrap_or(1)
}

fn render_unit_list(ui: &mut egui::Ui, state: &mut AppState) {
    ui.strong("Units");
    ui.separator();

    let raw = state.raw_source.as_mut().unwrap();
    let selected = raw.selected_unit;
    let mut clicked: Option<usize> = None;

    egui::ScrollArea::vertical()
        .id_source("raw_unit_list")
        .show(ui, |ui| {
            for (i, unit) in raw.units.iter().enumerate() {
                let label = format!("{}. {}", i + 1, unit.name);
                let response = ui.selectable_label(i == selected, label);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.colored_label(
                        status_color(unit.status),
                        format!("v{} {}", unit.version, unit.status.label()),
                    );
                    if let Some(r) = &unit.report {
                        ui.weak(format!(
                            "iL {:.1} · {} w · {:.0}%",
                            r.i_score.i_level, r.submission_words, r.percent_of_source
                        ));
                    }
                });
                if response.clicked() {
                    clicked = Some(i);
                }
                ui.separator();
            }
        });

    if let Some(i) = clicked {
        raw.selected_unit = i;
    }
}

fn render_unit_detail(ui: &mut egui::Ui, state: &mut AppState) {
    let raw = state.raw_source.as_ref().unwrap();
    let Some(unit) = raw.units.get(raw.selected_unit) else {
        ui.label("Select a unit.");
        return;
    };

    let raw_text = unit.source_text();
    let draft = unit.draft.clone();
    let name = unit.name.clone();
    let version = unit.version;
    let status = unit.status;
    let error = unit.last_error.clone();
    let report_text = unit
        .report
        .as_ref()
        .map(|r| crate::simulation::escore::render_report(r, &name));

    egui::ScrollArea::vertical()
        .id_source("raw_unit_detail")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(&name);
                ui.colored_label(status_color(status), format!("v{} {}", version, status.label()));
            });
            if let Some(err) = &error {
                ui.colored_label(FLOOR_COLOR, format!("error: {}", err));
            }
            ui.separator();

            ui.collapsing("Adapted draft", |ui| {
                if draft.trim().is_empty() {
                    ui.weak("No draft yet — press Draft or Run.");
                } else {
                    ui.label(egui::RichText::new(&draft).monospace());
                }
            })
            .header_response
            .on_hover_text("The first line is the chapter title used for %%META chapter:%%");

            ui.collapsing("DRC report", |ui| match &report_text {
                Some(text) => {
                    ui.label(egui::RichText::new(text).monospace());
                }
                None => {
                    ui.weak("No report yet — press Score.");
                }
            });

            ui.collapsing("Raw source", |ui| {
                ui.label(egui::RichText::new(&raw_text).monospace());
            });
        });
}
