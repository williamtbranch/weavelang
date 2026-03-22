// src/gui/components/media_view.rs
//
// Media tab: AV production status table, mark/unmark checkboxes, generation
// buttons, and config display. All mutations go through pending_terminal_command.

use crate::app::state::AppState;
use crate::services::av_producer::{AvFileStatus, AvProducer};
use eframe::egui;

/// Render the full Media tab content inside the central panel.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    // Resolve statuses from filesystem on each frame (cheap — just directory listing)
    let (statuses, manifest_ok, illustrations, error_msg) = load_statuses(state);

    // If a stem is selected and it has chunks, render the chunk detail panel on the right
    let selected = state.av_selected_stem.clone();
    let chunk_data = selected.as_ref().and_then(|stem| {
        load_chunk_data(state, stem)
    });

    // ---- Right: Chunk detail panel (reserve space first via SidePanel) ----
    let has_chunks = chunk_data.as_ref().map(|d| !d.chunks.is_empty()).unwrap_or(false);
    if has_chunks {
        if let (Some(stem), Some(data)) = (&selected, &chunk_data) {
            egui::SidePanel::right("media_chunk_panel")
                .resizable(true)
                .default_width(ui.available_width() * 0.35)
                .show_inside(ui, |ui| {
                    render_chunk_panel(ui, state, stem, data);
                });
        }
    }

    // ---- Bottom: batch buttons + job status (reserve space at bottom) ----
    egui::TopBottomPanel::bottom("media_bottom_bar")
        .show_inside(ui, |ui| {
            // --- AV job status bar ---
            if let Some(ref job) = state.av_job {
                let j = job.lock().unwrap();
                ui.horizontal(|ui| {
                    if j.finished {
                        ui.colored_label(
                            egui::Color32::from_rgb(80, 180, 80),
                            format!("Done: {}", j.label),
                        );
                    } else {
                        ui.colored_label(
                            egui::Color32::from_rgb(200, 180, 60),
                            format!("Running: {}", j.label),
                        );
                        ui.spinner();
                    }
                    if !j.finished {
                        drop(j);
                        if ui.button("Cancel").clicked() {
                            state.pending_terminal_command = Some("av cancel".to_string());
                        }
                    }
                });
                ui.separator();
            }

            // --- Batch buttons ---
            let job_running = state.av_job.as_ref()
                .map(|j| !j.lock().unwrap().finished)
                .unwrap_or(false);

            ui.horizontal(|ui| {
                if ui.button("Mark All").clicked() {
                    state.pending_terminal_command = Some("av mark-all".to_string());
                }
                if ui.button("Clear Marks").clicked() {
                    state.pending_terminal_command = Some("av clear-marks".to_string());
                }
                ui.separator();
                ui.add_enabled_ui(!job_running, |ui| {
                    if ui.button("Gen Next Audio").clicked() {
                        state.pending_terminal_command = Some("av generate audio next".to_string());
                    }
                    if ui.button("Gen Next Video").clicked() {
                        state.pending_terminal_command = Some("av generate video next".to_string());
                    }
                });
            });
        });

    // ---- Remaining space: heading + summary + file table (fills everything) ----
    render_main_panel(ui, state, &statuses, manifest_ok, illustrations, &error_msg);
}

// ---------------------------------------------------------------------------
// Chunk detail data
// ---------------------------------------------------------------------------

struct ChunkDetailData {
    chunks: Vec<crate::services::av_producer::ChunkStatus>,
    /// True if audio/<stem>.wav exists but some chunks are rejected or missing.
    stale_audio: bool,
}

fn load_chunk_data(state: &AppState, stem: &str) -> Option<ChunkDetailData> {
    let output_dir = state.output_dir.as_ref()?;
    let book_dir = AvProducer::resolve_book_dir(output_dir, &state.book_name);
    let producer = AvProducer::new(book_dir).ok()?;
    let chunks = producer.scan_chunks(stem);
    if chunks.is_empty() {
        return Some(ChunkDetailData { chunks, stale_audio: false });
    }
    let has_final_audio = producer.audio_dir()
        .join(format!("{}.{}", stem, producer.manifest.tts.output_format))
        .exists();
    let any_bad = chunks.iter().any(|c| c.is_rejected || (!c.has_audio && c.has_text));
    let stale_audio = has_final_audio && any_bad;
    Some(ChunkDetailData { chunks, stale_audio })
}

// ---------------------------------------------------------------------------
// Main panel (left side)
// ---------------------------------------------------------------------------

fn render_main_panel(
    ui: &mut egui::Ui,
    state: &mut AppState,
    statuses: &[AvFileStatus],
    manifest_ok: bool,
    illustrations: usize,
    error_msg: &Option<String>,
) {
    ui.heading("Media — AV Production");
    ui.add_space(4.0);

    if let Some(err) = error_msg {
        ui.colored_label(egui::Color32::from_rgb(200, 80, 80), err);
        ui.add_space(8.0);
        if ui.button("Initialize manifest (av init)").clicked() {
            state.pending_terminal_command = Some("av init".to_string());
        }
        return;
    }

    // --- Summary bar ---
    ui.horizontal(|ui| {
        let marked = statuses.iter().filter(|s| s.marked).count();
        let audio_done = statuses.iter().filter(|s| s.marked && s.has_audio).count();
        let video_done = statuses.iter().filter(|s| s.marked && s.has_video).count();
        ui.label(format!(
            "{} files | {} marked | Audio: {}/{} | Video: {}/{}",
            statuses.len(), marked, audio_done, marked, video_done, marked
        ));

        ui.separator();
        ui.label(format!("Illustrations: {}", illustrations));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Open book dir").clicked() {
                state.pending_terminal_command = Some("av open book-dir".to_string());
            }
            if ui.small_button("Open audio").clicked() {
                state.pending_terminal_command = Some("av open audio-dir".to_string());
            }
            if ui.small_button("Open video").clicked() {
                state.pending_terminal_command = Some("av open video-dir".to_string());
            }
            if ui.small_button("Open illustrations").clicked() {
                state.pending_terminal_command = Some("av open illustrations".to_string());
            }
        });
    });

    ui.separator();

    // --- Config summary ---
    if manifest_ok {
        render_config_summary(ui, state);
        ui.separator();
    }

    // --- File status table (fills all remaining vertical space) ---
    if statuses.is_empty() {
        ui.label("No woven text files found in the book directory.");
    } else {
        let job_active = state.av_job.as_ref()
            .map(|j| !j.lock().unwrap().finished)
            .unwrap_or(false);
        render_status_table(ui, state, statuses, illustrations, job_active);
    }
}

// ---------------------------------------------------------------------------
// Chunk detail panel (right side)
// ---------------------------------------------------------------------------

fn render_chunk_panel(
    ui: &mut egui::Ui,
    state: &mut AppState,
    stem: &str,
    data: &ChunkDetailData,
) {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.strong(format!("Chunks: {}", stem));
            if ui.small_button("✕").clicked() {
                state.av_selected_stem = None;
            }
        });

        if data.stale_audio {
            ui.horizontal(|ui| {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 160, 40),
                    "⚠ Final audio is stale — rebuild after fixing chunks",
                );
                if ui.button("Rebuild").clicked() {
                    state.pending_terminal_command =
                        Some(format!("av rebuild audio {}", stem));
                }
            });
        }

        let good = data.chunks.iter().filter(|c| c.has_audio && !c.is_rejected).count();
        let rejected = data.chunks.iter().filter(|c| c.is_rejected).count();
        let missing = data.chunks.iter().filter(|c| !c.has_audio && !c.is_rejected && c.has_text).count();
        ui.label(format!(
            "{} chunks | {} good | {} rejected | {} missing",
            data.chunks.len(), good, rejected, missing
        ));

        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("chunk_detail_grid")
                    .striped(true)
                    .num_columns(4)
                    .min_col_width(30.0)
                    .show(ui, |ui| {
                        ui.strong("#");
                        ui.strong("Text");
                        ui.strong("Audio");
                        ui.strong("Action");
                        ui.end_row();

                        for c in &data.chunks {
                            // Index
                            ui.label(format!("{:>4}", c.index));

                            // Text status
                            if c.has_text {
                                ui.colored_label(egui::Color32::from_rgb(80, 180, 80), "✓");
                            } else {
                                ui.label("—");
                            }

                            // Audio status with color
                            if c.is_rejected {
                                ui.colored_label(egui::Color32::from_rgb(200, 80, 80), "✗ bad");
                            } else if c.has_audio {
                                ui.colored_label(egui::Color32::from_rgb(80, 180, 80), "✓");
                            } else {
                                ui.colored_label(egui::Color32::from_rgb(140, 140, 140), "—");
                            }

                            // Action buttons
                            if c.is_rejected {
                                if ui.small_button("Restore").clicked() {
                                    state.pending_terminal_command = Some(format!(
                                        "av restore chunk {} {}", stem, c.index
                                    ));
                                }
                            } else if c.has_audio {
                                if ui.small_button("Reject").clicked() {
                                    state.pending_terminal_command = Some(format!(
                                        "av reject chunk {} {}", stem, c.index
                                    ));
                                }
                            } else {
                                ui.label("—");
                            }

                            ui.end_row();
                        }
                    });
            });
    });
}

/// Load AV statuses from the filesystem. Returns (statuses, manifest_loaded, illustration_count, error).
fn load_statuses(state: &AppState) -> (Vec<AvFileStatus>, bool, usize, Option<String>) {
    let output_dir = match state.output_dir.as_ref() {
        Some(d) => d,
        None => {
            return (
                Vec::new(),
                false,
                0,
                Some("Output directory not set. Use 'set output_dir <path>'.".to_string()),
            )
        }
    };

    let book_dir = AvProducer::resolve_book_dir(output_dir, &state.book_name);

    // In chapter mode, point to the selected chapter's subdirectory
    let target_dir = if state.chapter_mode {
        if let Some(ch_idx) = state.selected_chapter_idx {
            if let Some(ch) = state.chapters.get(ch_idx) {
                let ch_dir_name = ch.name
                    .replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && c != ' ', "")
                    .trim().replace(' ', "_");
                book_dir.join(ch_dir_name)
            } else {
                book_dir	
            }
        } else {
            return (
                Vec::new(),
                false,
                0,
                Some("Chapter mode active but no chapter selected. Use 'select chapter \"<name>\"'.".to_string()),
            );
        }
    } else {
        book_dir
    };

    if !target_dir.exists() {
        return (
            Vec::new(),
            false,
            0,
            Some(format!(
                "Directory not found: {}. Run 'init media' to create the workspace.",
                target_dir.display()
            )),
        );
    }

    match AvProducer::new(target_dir) {
        Ok(producer) => {
            let statuses = producer.scan();
            let illustrations = producer.count_illustrations();
            (statuses, true, illustrations, None)
        }
        Err(e) => (Vec::new(), false, 0, Some(e)),
    }
}

/// Render a compact config summary line with an expand button.
fn render_config_summary(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        // Try to load manifest to show config
        if let Some(output_dir) = state.output_dir.as_ref() {
            let book_dir = AvProducer::resolve_book_dir(output_dir, &state.book_name);
            if let Ok(producer) = AvProducer::new(book_dir) {
                let tts = &producer.manifest.tts;
                let voices_str = if tts.voices.len() <= 3 {
                    tts.voices.join(", ")
                } else {
                    format!(
                        "{}, {} +{} more",
                        tts.voices[0],
                        tts.voices[1],
                        tts.voices.len() - 2
                    )
                };
                ui.label(format!(
                    "TTS: {} / {} | Voices: {} | Chunk: {} chars",
                    tts.service, tts.model, voices_str, tts.chunk_max_chars
                ));

                let vid = &producer.manifest.video;
                ui.separator();
                ui.label(format!(
                    "Video: {}s/image, {} fps",
                    vid.image_duration, vid.frame_rate
                ));
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Config...").clicked() {
                state.pending_terminal_command = Some("av config show".to_string());
            }
        });
    });
}

/// Render the file status table with checkboxes and action buttons.
fn render_status_table(
    ui: &mut egui::Ui,
    state: &mut AppState,
    statuses: &[AvFileStatus],
    illustration_count: usize,
    job_running: bool,
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("av_status_grid")
                .striped(true)
                .num_columns(6)
                .min_col_width(40.0)
                .show(ui, |ui| {
                    // Header
                    ui.strong("Mark");
                    ui.strong("File");
                    ui.strong("Text");
                    ui.strong("Audio");
                    ui.strong("Video");
                    ui.strong("Action");
                    ui.end_row();

                    for s in statuses {
                        // Checkbox (mark/unmark)
                        let mut marked = s.marked;
                        if ui.checkbox(&mut marked, "").changed() {
                            if marked {
                                state.pending_terminal_command =
                                    Some(format!("av mark {}", s.stem));
                            } else {
                                state.pending_terminal_command =
                                    Some(format!("av unmark {}", s.stem));
                            }
                        }

                        // Stem name — clickable to show chunk detail panel
                        let is_selected = state.av_selected_stem.as_deref() == Some(&s.stem);
                        let label = if is_selected {
                            egui::RichText::new(&s.stem).strong()
                        } else {
                            egui::RichText::new(&s.stem)
                        };
                        if ui.selectable_label(is_selected, label).clicked() {
                            if is_selected {
                                state.av_selected_stem = None;
                            } else {
                                state.av_selected_stem = Some(s.stem.clone());
                            }
                        }

                        // Text status
                        status_icon(ui, s.has_text, true);

                        // Audio status — check for stale indicator
                        let chunk_stale = s.has_audio && {
                            load_chunk_data(state, &s.stem)
                                .map(|d| d.stale_audio)
                                .unwrap_or(false)
                        };
                        if chunk_stale {
                            ui.colored_label(egui::Color32::from_rgb(220, 160, 40), "⚠");
                        } else {
                            status_icon(ui, s.has_audio, s.marked);
                        }

                        // Video status
                        let video_relevant = s.marked && s.has_audio;
                        status_icon(ui, s.has_video, video_relevant);

                        // Action button
                        if s.marked {
                            if !s.has_audio {
                                if !job_running && ui.small_button("Gen Audio").clicked() {
                                    state.pending_terminal_command = Some(format!(
                                        "av generate audio {}",
                                        s.stem
                                    ));
                                } else if job_running {
                                    ui.label("...");
                                }
                            } else if !s.has_video {
                                if illustration_count > 0 {
                                    if !job_running && ui.small_button("Gen Video").clicked() {
                                        state.pending_terminal_command = Some(format!(
                                            "av generate video {}",
                                            s.stem
                                        ));
                                    } else if job_running {
                                        ui.label("...");
                                    }
                                } else {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(180, 140, 60),
                                        "Need illustrations",
                                    );
                                }
                            } else {
                                ui.colored_label(
                                    egui::Color32::from_rgb(80, 180, 80),
                                    "Done",
                                );
                            }
                        } else {
                            ui.label("—");
                        }

                        ui.end_row();
                    }
                });
        });
}

/// Draw a status icon: check for present, X for missing-but-relevant, dash for N/A.
fn status_icon(ui: &mut egui::Ui, present: bool, relevant: bool) {
    if present {
        ui.colored_label(egui::Color32::from_rgb(80, 180, 80), "✓");
    } else if relevant {
        ui.colored_label(egui::Color32::from_rgb(200, 80, 80), "✗");
    } else {
        ui.label("—");
    }
}
