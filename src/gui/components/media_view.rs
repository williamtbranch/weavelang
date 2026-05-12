// src/gui/components/media_view.rs
//
// Media tab: AV production status table, mark/unmark checkboxes, generation
// buttons, and config display. All mutations go through pending_terminal_command.

use crate::app::state::AppState;
use crate::services::av_producer::{
    AvFileStatus,
    AvProducer,
    SF_ALIGNMENT_MAP_FILENAME,
    SF_CHUNK_META_FILENAME,
};
use eframe::egui;

struct SfCardStatus {
    enabled: bool,
    source_level: String,
    levels: Vec<String>,
    sf_stem: String,
    sf_audio_exists: bool,
    sf_video_exists: bool,
    missing_items: Vec<String>,
    missing_meta_count: usize,
}

/// Resolve the target directory for AV operations.
/// In chapter mode returns `<book_dir>/<chapter_name>/`, otherwise `<book_dir>/whole_book/`.
fn resolve_target_dir(state: &AppState) -> Option<std::path::PathBuf> {
    let output_dir = state.output_dir.as_ref()?;
    let book_dir = AvProducer::resolve_book_dir(output_dir, &state.book_name);
    if state.chapter_mode {
        let ch_idx = state.selected_chapter_idx?;
        let ch = state.chapters.get(ch_idx)?;
        let ch_dir_name = ch.name
            .replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && c != ' ', "")
            .trim().replace(' ', "_");
        Some(book_dir.join(ch_dir_name))
    } else {
        Some(book_dir.join("whole_book"))
    }
}

/// Render the full Media tab content inside the central panel.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    let show_align = state.chapter_mode
        && state.lesson_realign_enabled
        && state.source_is_target();

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
                    if show_align && ui.button("Gen Next Align").clicked() {
                        state.pending_terminal_command = Some("av generate align next".to_string());
                    }
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
    render_main_panel(ui, state, &statuses, manifest_ok, illustrations, &error_msg, show_align);
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
    let target_dir = resolve_target_dir(state)?;
    let producer = AvProducer::new(target_dir).ok()?;
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
    show_align: bool,
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
        let align_done = statuses.iter().filter(|s| s.marked && s.has_aligned_text).count();
        let audio_done = statuses.iter().filter(|s| s.marked && s.has_audio).count();
        let video_done = statuses.iter().filter(|s| s.marked && s.has_video).count();
        let summary = if show_align {
            format!(
                "{} files | {} marked | Align: {}/{} | Audio: {}/{} | Video: {}/{}",
                statuses.len(), marked, align_done, marked, audio_done, marked, video_done, marked
            )
        } else {
            format!(
                "{} files | {} marked | Audio: {}/{} | Video: {}/{}",
                statuses.len(), marked, audio_done, marked, video_done, marked
            )
        };
        ui.label(summary);

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
        render_sf_card(ui, state, illustrations);
        ui.separator();
    }

    // --- File status table (fills all remaining vertical space) ---
    if statuses.is_empty() {
        ui.label("No woven text files found in the book directory.");
    } else {
        let job_active = state.av_job.as_ref()
            .map(|j| !j.lock().unwrap().finished)
            .unwrap_or(false);
        render_status_table(ui, state, statuses, illustrations, job_active, show_align);
    }
}

fn render_sf_card(ui: &mut egui::Ui, state: &mut AppState, illustrations: usize) {
    let job_running = state.av_job.as_ref()
        .map(|j| !j.lock().unwrap().finished)
        .unwrap_or(false);

    let sf_status = load_sf_card_status(state);
    let Some(sf) = sf_status else {
        return;
    };

    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.strong("Study Format");
            if sf.enabled {
                ui.colored_label(egui::Color32::from_rgb(80, 180, 80), "enabled");
            } else {
                ui.colored_label(egui::Color32::from_rgb(180, 120, 60), "disabled");
            }
            ui.separator();
            ui.label(format!("source: UL{}", sf.source_level));
            ui.separator();
            ui.label(format!("levels: {}", sf.levels.join(", ")));
            ui.separator();
            ui.label(format!("stem: {}", sf.sf_stem));
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            status_icon(ui, sf.sf_audio_exists, true);
            ui.label("SF audio");
            ui.separator();
            status_icon(ui, sf.sf_video_exists, sf.sf_audio_exists);
            ui.label("SF video");
            ui.separator();

            let prereq_ok = sf.enabled && sf.missing_items.is_empty();
            if prereq_ok {
                ui.colored_label(egui::Color32::from_rgb(80, 180, 80), "Ready to build");
            } else {
                ui.colored_label(egui::Color32::from_rgb(200, 80, 80), "Prereqs missing");
            }
        });

        if !sf.missing_items.is_empty() {
            ui.add_space(2.0);
            for miss in sf.missing_items.iter().take(4) {
                ui.colored_label(egui::Color32::from_rgb(200, 80, 80), format!("- {}", miss));
            }
            if sf.missing_items.len() > 4 {
                ui.colored_label(
                    egui::Color32::from_rgb(200, 80, 80),
                    format!("- ... and {} more", sf.missing_items.len() - 4),
                );
            }
        }

        if sf.missing_meta_count > 0 {
            ui.colored_label(
                egui::Color32::from_rgb(180, 140, 60),
                format!("Warning: {} chunk set(s) missing _sf_chunk_meta.json", sf.missing_meta_count),
            );
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.small_button("Preflight").clicked() {
                state.pending_terminal_command = Some("av sf preflight".to_string());
            }

            let can_build = sf.enabled && sf.missing_items.is_empty() && !sf.sf_audio_exists && !job_running;
            if ui.add_enabled(can_build, egui::Button::new("Build SF Audio")).clicked() {
                state.pending_terminal_command = Some("av sf build next".to_string());
            }

            let can_video = sf.sf_audio_exists && !sf.sf_video_exists && illustrations > 0 && !job_running;
            if ui.add_enabled(can_video, egui::Button::new("Gen SF Video")).clicked() {
                state.pending_terminal_command = Some(format!("av generate video {}", sf.sf_stem));
            }

            if sf.sf_audio_exists && sf.sf_video_exists {
                ui.colored_label(egui::Color32::from_rgb(80, 180, 80), "Done");
            } else if sf.sf_audio_exists && !sf.sf_video_exists && illustrations == 0 {
                ui.colored_label(egui::Color32::from_rgb(180, 140, 60), "Need illustrations");
            }
        });
    });
}

fn load_sf_card_status(state: &AppState) -> Option<SfCardStatus> {
    let target_dir = resolve_target_dir(state)?;
    let producer = AvProducer::new(target_dir).ok()?;

    let sf_cfg = &producer.manifest.study_format;
    let tts_ext = producer.manifest.tts.output_format.clone();
    let sf_stem = format!("{}ULsf", state.book_name);

    let audio_dir = producer.audio_dir();
    let chunks_root = audio_dir.join("chunks");
    let alignment_path = audio_dir.join(SF_ALIGNMENT_MAP_FILENAME);

    let mut missing_items = Vec::new();
    if !alignment_path.exists() {
        missing_items.push(format!("Missing {}", SF_ALIGNMENT_MAP_FILENAME));
    }

    let mut missing_meta_count = 0usize;

    let mut all_suffixes = Vec::with_capacity(sf_cfg.levels.len() + 1);
    all_suffixes.push(sf_cfg.source_level.clone());
    all_suffixes.extend(sf_cfg.levels.iter().cloned());

    for suffix in &all_suffixes {
        let chunk_dir = find_chunk_dir_for_level(&chunks_root, &state.book_name, suffix);
        match chunk_dir {
            Some(dir) => {
                let meta_path = dir.join(SF_CHUNK_META_FILENAME);
                if !meta_path.exists() {
                    missing_meta_count += 1;
                }
            }
            None => {
                missing_items.push(format!("Missing chunk dir for UL{}", suffix));
            }
        }
    }

    let sf_audio_exists = audio_dir.join(format!("{}.{}", sf_stem, tts_ext)).exists();
    let sf_video_exists = producer.video_dir().join(format!("{}.mp4", sf_stem)).exists();

    Some(SfCardStatus {
        enabled: sf_cfg.enabled,
        source_level: sf_cfg.source_level.clone(),
        levels: sf_cfg.levels.clone(),
        sf_stem,
        sf_audio_exists,
        sf_video_exists,
        missing_items,
        missing_meta_count,
    })
}

fn find_chunk_dir_for_level(chunks_root: &std::path::Path, book_name: &str, level_suffix: &str) -> Option<std::path::PathBuf> {
    if !chunks_root.exists() {
        return None;
    }

    let suffix = format!("UL{}", level_suffix);
    let preferred = chunks_root.join(format!("{}{}", book_name, suffix));
    if preferred.is_dir() {
        return Some(preferred);
    }

    std::fs::read_dir(chunks_root)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.ends_with(&suffix))
                    .unwrap_or(false)
        })
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
        book_dir.join("whole_book")
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
                    "Video: {}s/image, {} fps{}",
                    vid.image_duration, vid.frame_rate,
                    if vid.max_sentences_per_video > 0 {
                        format!(", max {}/vol", vid.max_sentences_per_video)
                    } else {
                        String::new()
                    }
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
    show_align: bool,
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("av_status_grid")
                .striped(true)
                .num_columns(if show_align { 7 } else { 6 })
                .min_col_width(40.0)
                .show(ui, |ui| {
                    // Header
                    ui.strong("Mark");
                    ui.strong("File");
                    ui.strong("Text");
                    if show_align {
                        ui.strong("Align");
                    }
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

                        if show_align {
                            status_icon(ui, s.has_aligned_text, s.marked && s.has_text);
                        }

                        // Audio status — check for stale indicator + volume info
                        let chunk_stale = s.has_audio && {
                            load_chunk_data(state, &s.stem)
                                .map(|d| d.stale_audio)
                                .unwrap_or(false)
                        };
                        if chunk_stale {
                            ui.colored_label(egui::Color32::from_rgb(220, 160, 40), "⚠");
                        } else if s.volume_count > 0 {
                            ui.colored_label(
                                egui::Color32::from_rgb(80, 180, 80),
                                format!("✓ V{}", s.volume_count),
                            );
                        } else {
                            status_icon(ui, s.has_audio, s.marked);
                        }

                        // Video status — volume-aware
                        let video_relevant = s.marked && s.has_audio;
                        if s.volume_count > 0 && s.has_audio {
                            if s.volumes_with_video == s.volume_count {
                                ui.colored_label(
                                    egui::Color32::from_rgb(80, 180, 80),
                                    format!("✓ {}/{}", s.volumes_with_video, s.volume_count),
                                );
                            } else {
                                ui.colored_label(
                                    egui::Color32::from_rgb(200, 80, 80),
                                    format!("{}/{}", s.volumes_with_video, s.volume_count),
                                );
                            }
                        } else {
                            status_icon(ui, s.has_video, video_relevant);
                        }

                        // Action button
                        if s.marked {
                            if show_align && !s.has_aligned_text {
                                if !job_running && ui.small_button("Gen Align").clicked() {
                                    state.pending_terminal_command = Some(format!(
                                        "av generate align {}",
                                        s.stem
                                    ));
                                } else if job_running {
                                    ui.label("...");
                                }
                            } else if !s.has_audio {
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
