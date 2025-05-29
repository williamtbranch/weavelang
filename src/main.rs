//*** START FILE: src/main.rs ***//
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// --- Standard Library Imports ---
use std::collections::HashMap;
use std::error::Error;
use std::fs; // Renamed from std_fs for direct use
use std::path::PathBuf;

// --- External Crate Imports ---
use clap::Parser;
use eframe::{egui, App as EframeApp, NativeOptions};

// --- Crate-Specific Imports (from our library `weavelang_rust_gui`) ---
use weavelang_rust_gui::config::{self, Config}; // Import specific item and module
use weavelang_rust_gui::corpus_generator;
// profile_io is used by corpus_generator

// For the GUI (WeaveLangApp and its methods)
use weavelang_rust_gui::types::llm_data::{
    ProcessedChapter as GuiStringProcessedChapter,
    ProcessedSentence as GuiStringProcessedSentence, // If used by GUI state/methods
};
use weavelang_rust_gui::simulation::dictionary::GlobalLemmaDictionary as GuiGlobalLemmaDictionary;
use weavelang_rust_gui::simulation::numerical_types::{
    NumericalChapter as GuiNumericalChapter,
    NumericalLearnerProfile as GuiNumericalLearnerProfile,
    NumericalProcessedSentence as GuiNumericalProcessedSentence, // For Vec type in orchestrator
};
use weavelang_rust_gui::profile::LemmaState as GuiLemmaState; // For orchestrator logic


// --- CLI Argument Structures ---
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(short, long, value_name = "FILE", default_value = "config.toml")]
    config: PathBuf,
}

#[derive(Parser, Debug)]
enum Commands {
    Gui,
    Generate(GenerateCliArgs),
}

#[derive(Parser, Debug, Clone)]
struct GenerateCliArgs {
    #[arg(short, long, value_name = "FILE")]
    sequence: PathBuf,
    #[arg(long, value_name = "DIR", default_value = "./tts_output")]
    tts_output_dir: PathBuf,
    #[arg(long, value_name = "DIR", default_value = "./profiles")]
    profiles_dir: PathBuf,
    #[arg(long, value_name = "FILE")]
    start_profile: Option<PathBuf>,
    #[arg(long, default_value_t = 200)]
    sentences_per_block: usize,
    #[arg(long, default_value_t = 25)]
    max_regen_attempts_per_block: u32,
    #[arg(long, default_value_t = 0.98)]
    target_ct_threshold: f32,
    #[arg(long, default_value_t = 3)]
    max_words_to_activate_per_regen: usize,
}

// --- GUI Application (WeaveLangApp struct) ---
struct WeaveLangApp {
    config: Option<Config>,
    config_error: Option<String>,
    content_path_display: String,
    stage_files: Vec<PathBuf>,
    selected_stage_file: Option<PathBuf>,
    selected_file_content: String,
    current_string_chapter: Option<GuiStringProcessedChapter>,
    current_numerical_chapter: Option<GuiNumericalChapter>,
    global_lemma_dictionary: GuiGlobalLemmaDictionary,
    learner_profile: GuiNumericalLearnerProfile,
    parser_display_error: Option<String>,
    scan_error: Option<String>,
    processed_json_output: String,
    woven_text_output: String,
    simulation_log_output: String,
    generation_error: Option<String>,
    sentences_per_block: usize,
    max_simulation_loops: u32,
    max_regen_attempts_per_block: u32,
    target_ct_threshold: f32,
    max_words_to_activate_per_regen: usize,
}

impl WeaveLangApp {
    fn new(
        _cc: &eframe::CreationContext<'_>,
        app_config: Option<Config>,
        config_error_msg: Option<String>,
    ) -> Self {
        let content_path_display_val = match &app_config {
            Some(conf) => format!("Content Dir: {}", conf.content_project_dir),
            None => config_error_msg.clone().unwrap_or_else(|| "Config not loaded or error during load.".to_string()),
        };
        Self {
            config: app_config,
            config_error: config_error_msg,
            content_path_display: content_path_display_val,
            stage_files: Vec::new(),
            selected_stage_file: None,
            selected_file_content: String::new(),
            current_string_chapter: None,
            current_numerical_chapter: None,
            global_lemma_dictionary: GuiGlobalLemmaDictionary::new(),
            learner_profile: GuiNumericalLearnerProfile::new(),
            parser_display_error: None,
            scan_error: None,
            processed_json_output: String::new(),
            woven_text_output: String::new(),
            simulation_log_output: String::new(),
            generation_error: None,
            sentences_per_block: 100,
            max_simulation_loops: 10,
            max_regen_attempts_per_block: 25,
            target_ct_threshold: 0.98,
            max_words_to_activate_per_regen: 3,
        }
    }

    fn reset_chapter_specific_data(&mut self) {
        self.selected_file_content.clear();
        self.current_string_chapter = None;
        self.current_numerical_chapter = None;
        self.processed_json_output.clear();
        self.parser_display_error = None;
        self.generation_error = None;
    }

    fn reset_simulation_outputs(&mut self) {
        self.woven_text_output.clear();
        self.simulation_log_output.clear();
        self.generation_error = None;
    }

    fn scan_stage_directory(&mut self) {
        self.stage_files.clear();
        self.selected_stage_file = None;
        self.scan_error = None;
        self.reset_chapter_specific_data();
        self.reset_simulation_outputs();

        if let Some(conf) = &self.config {
            let stage_path = PathBuf::from(&conf.content_project_dir).join("stage");
            if !stage_path.is_dir() {
                self.scan_error = Some(format!("Stage directory not found: {:?}", stage_path));
                return;
            }
            match fs::read_dir(stage_path) { // Using fs directly from `use std::fs;`
                Ok(entries) => {
                    for entry in entries {
                        if let Ok(entry) = entry {
                            let path = entry.path();
                            if path.is_file() {
                                if let Some(name_str) = path.file_name().and_then(|n| n.to_str()) {
                                    if name_str.ends_with(".llm.txt") {
                                        self.stage_files.push(path);
                                    }
                                }
                            }
                        }
                    }
                    if self.stage_files.is_empty() {
                        self.scan_error = Some("No .llm.txt files found.".to_string());
                    }
                    self.stage_files.sort();
                }
                Err(e) => { self.scan_error = Some(format!("Failed to read stage directory: {}", e)); }
            }
        } else { self.scan_error = Some("Config not loaded.".to_string()); }
    }

    fn load_and_parse_selected_file(&mut self, path_to_load: &PathBuf) {
        self.reset_chapter_specific_data();
        self.reset_simulation_outputs();
        self.selected_stage_file = Some(path_to_load.clone());

        match fs::read_to_string(path_to_load) { // Using fs directly
            Ok(contents) => {
                self.selected_file_content = contents.clone();
                let file_name = path_to_load.file_name().unwrap_or_default().to_string_lossy().into_owned();

                match weavelang_rust_gui::parsing::llm_parser::parse_llm_text_to_chapter(&file_name, &contents) {
                    Ok(parsed_string_chapter) => {
                        // Populate GUI's dictionary instance
                        self.global_lemma_dictionary.populate_from_chapter(&parsed_string_chapter);
                        let numerical_version = weavelang_rust_gui::simulation::preprocessor::to_numerical_chapter(
                            &parsed_string_chapter,
                            &mut self.global_lemma_dictionary,
                        );

                        if !parsed_string_chapter.sentences.is_empty() {
                            let new_spb = (parsed_string_chapter.sentences.len()).max(1).min(5000); // ensure it's at least 1, max 5000
                            if new_spb != self.sentences_per_block {
                                self.simulation_log_output.push_str(&format!(
                                    "[INFO] GUI: Auto-adjusted sentences_per_block from {} to {} for chapter '{}'.\n",
                                    self.sentences_per_block, new_spb, file_name
                                ));
                                self.sentences_per_block = new_spb;
                            }
                        }

                        self.current_string_chapter = Some(parsed_string_chapter.clone());
                        self.current_numerical_chapter = Some(numerical_version);

                        match serde_json::to_string_pretty(&parsed_string_chapter) {
                            Ok(json_string) => self.processed_json_output = json_string,
                            Err(e) => self.parser_display_error = Some(format!("JSON Serialization failed: {}", e)),
                        }
                    }
                    Err(e) => {
                        self.parser_display_error = Some(format!("Parser Error for {}: {}", file_name, e));
                    }
                }
            }
            Err(e) => {
                self.parser_display_error = Some(format!("Error loading file {:?}: {}", path_to_load.file_name().unwrap_or_default(), e));
            }
        }
    }

    fn run_simulation_orchestrator(&mut self) {
        self.reset_simulation_outputs();

        let numerical_chapter_ref: &GuiNumericalChapter = match &self.current_numerical_chapter {
            Some(nc_ref) => nc_ref,
            None => {
                self.simulation_log_output.push_str("\nERROR: Numerical chapter not loaded for simulation.");
                self.generation_error = Some("Numerical chapter is not loaded. Please load a file first.".to_string());
                return;
            }
        };

        let string_chapter_ref: &GuiStringProcessedChapter = match &self.current_string_chapter {
            Some(sc_ref) => sc_ref,
            None => {
                self.simulation_log_output.push_str("\nERROR: String chapter not loaded for simulation.");
                self.generation_error = Some("String chapter is not loaded. Please load a file first.".to_string());
                return;
            }
        };

        if numerical_chapter_ref.sentences_numerical.is_empty() {
            self.generation_error = Some("GUI: Current numerical chapter has no sentences.".to_string());
            self.simulation_log_output.push_str("\nERROR: Numerical chapter has no sentences.");
            return;
        }

        let mut accumulated_log_for_display: Vec<String> = Vec::new();
        let mut accumulated_woven_text_for_display: String = String::new();

        let initial_profile_stats = format!(
            "INITIAL PROFILE for Run: Known: {}, Active (only): {}, Total K/A: {}, Vocab Size (Profile): {}, Global Dict Size: {}, Total Exposures: {}\n",
            self.learner_profile.count_known(), self.learner_profile.count_active_only(),
            self.learner_profile.count_total_known_or_active(), self.learner_profile.vocabulary_size(),
            self.global_lemma_dictionary.size(), self.learner_profile.total_exposure_count()
        );
        accumulated_log_for_display.push(initial_profile_stats.clone());
        accumulated_woven_text_for_display.push_str(&format!("%%WEAVELANG_STAT%% {}", initial_profile_stats));

        let total_sentences_in_source_chapter = numerical_chapter_ref.sentences_numerical.len();
        let mut overall_sentences_processed_this_run = 0;
        let mut current_source_sentence_idx = 0;
        let total_sentences_to_simulate_overall = total_sentences_in_source_chapter * self.max_simulation_loops as usize;
        let mut measurement_block_counter = 0;

        while overall_sentences_processed_this_run < total_sentences_to_simulate_overall {
            measurement_block_counter += 1;
            accumulated_log_for_display.push(format!(
                "\n--- GUI Orchestrator: Preparing Measurement Block {} ---",
                measurement_block_counter
            ));

            let mut block_numerical_sentences_refs: Vec<&GuiNumericalProcessedSentence> = Vec::new();
            let mut block_string_sentences_refs: Vec<&GuiStringProcessedSentence> = Vec::new();

            for _ in 0..self.sentences_per_block {
                if overall_sentences_processed_this_run >= total_sentences_to_simulate_overall { break; }
                if current_source_sentence_idx >= total_sentences_in_source_chapter { break; } // Safety for empty chapters after first loop

                block_numerical_sentences_refs.push(&numerical_chapter_ref.sentences_numerical[current_source_sentence_idx]);
                if current_source_sentence_idx < string_chapter_ref.sentences.len() {
                    block_string_sentences_refs.push(&string_chapter_ref.sentences[current_source_sentence_idx]);
                } else {
                    eprintln!("Mismatch between numerical and string sentence counts in GUI orchestrator!");
                    break;
                }
                
                current_source_sentence_idx = (current_source_sentence_idx + 1) % total_sentences_in_source_chapter;
                if total_sentences_in_source_chapter == 0 { break; } // Avoid infinite loop on empty chapter after first pass
                overall_sentences_processed_this_run += 1;
            }

            if block_numerical_sentences_refs.is_empty() {
                accumulated_log_for_display.push("GUI Orchestrator: No more sentences to form a new block. Ending run.".to_string());
                break;
            }

            accumulated_log_for_display.push(format!(
                "GUI Orchestrator: Calling core_algo for block {} ({} sentences). Profile K: {}, A: {}",
                measurement_block_counter,
                block_numerical_sentences_refs.len(),
                self.learner_profile.count_known(),
                self.learner_profile.count_active_only()
            ));

            let mut block_new_lemma_freq: HashMap<u32, u32> = HashMap::new();
            for num_sentence_ref in &block_numerical_sentences_refs {
                let mut sentence_lemma_ids_for_freq_check : Vec<u32> = Vec::new();
                sentence_lemma_ids_for_freq_check.extend(&num_sentence_ref.adv_s_lemma_ids);
                for nsl in &num_sentence_ref.sim_s_lemmas_numerical {
                    sentence_lemma_ids_for_freq_check.extend(&nsl.lemma_ids);
                }
                for ndsm in &num_sentence_ref.diglot_map_numerical {
                    for nde in &ndsm.entries {
                        if nde.viable { sentence_lemma_ids_for_freq_check.push(nde.spa_lemma_id); }
                    }
                }
                for &lemma_id in &sentence_lemma_ids_for_freq_check {
                    if self.learner_profile.get_lemma_info(lemma_id).map_or(true, |info| info.state == GuiLemmaState::New) {
                        *block_new_lemma_freq.entry(lemma_id).or_insert(0) += 1;
                    }
                }
            }
            let mut sorted_block_specific_new_lemma_ids_for_activation: Vec<(u32, u32)> = block_new_lemma_freq.into_iter().collect();
            sorted_block_specific_new_lemma_ids_for_activation.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

            match weavelang_rust_gui::simulation::core_algo::run_simulation_numerical(
                &block_numerical_sentences_refs,
                self.learner_profile.clone(),
                &sorted_block_specific_new_lemma_ids_for_activation,
                self.max_regen_attempts_per_block,
                self.target_ct_threshold,
                self.max_words_to_activate_per_regen,
            ) {
                Ok(block_simulation_result) => {
                    accumulated_log_for_display.extend(block_simulation_result.simulation_log_entries.clone());
                    // Important: Update the app's main learner_profile for the GUI simulation
                    self.learner_profile = block_simulation_result.profile_state_after_block_exposure;

                    match weavelang_rust_gui::simulation::text_generator::generate_final_text_block(
                        &block_string_sentences_refs,
                        &self.global_lemma_dictionary, // Use GUI's dictionary
                        &block_simulation_result.profile_state_for_text_generation,
                    ) {
                        Ok(generated_text_for_block) => {
                            accumulated_woven_text_for_display.push_str(&generated_text_for_block);
                            if !generated_text_for_block.trim().is_empty() && !accumulated_woven_text_for_display.ends_with("\n\n") {
                                 accumulated_woven_text_for_display.push_str("\n\n");
                            }
                            // ... (stat logging as before) ...
                        }
                        Err(e_text_gen) => {
                            let err_msg = format!("[GUI Orchestrator Error] Text generation for block {}: {}", measurement_block_counter, e_text_gen);
                            accumulated_log_for_display.push(err_msg.clone());
                            self.generation_error = Some(err_msg);
                            break;
                        }
                    }
                    // ... (log profile stats after block) ...
                }
                Err(e_sim) => {
                    let err_msg = format!("[GUI Orchestrator Error] Core simulation for block {}: {}", measurement_block_counter, e_sim);
                    accumulated_log_for_display.push(err_msg.clone());
                    self.generation_error = Some(err_msg);
                    break;
                }
            }
            if overall_sentences_processed_this_run >= total_sentences_to_simulate_overall { break; }
            // ... (log end of block / start of next block) ...
        }
        self.simulation_log_output = accumulated_log_for_display.join("\n");
        self.woven_text_output = accumulated_woven_text_for_display.trim_end().to_string();
    }
}

impl EframeApp for WeaveLangApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // This is the FULL GUI layout from your previous working version
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Profile", |ui| {
                    if ui.button("Reset Learner Profile & Global Dictionary").clicked() {
                        self.learner_profile = GuiNumericalLearnerProfile::new();
                        self.global_lemma_dictionary = GuiGlobalLemmaDictionary::new();
                        self.reset_simulation_outputs();
                        self.reset_chapter_specific_data();
                        self.selected_stage_file = None;
                    }
                });
            });
        });

        egui::SidePanel::left("side_panel_left")
            .min_width(250.0)
            .default_width(350.0)
            .show(ctx, |ui| {
                ui.heading("Controls & Info");
                ui.separator();
                ui.collapsing("Configuration", |ui| {
                    if let Some(err) = &self.config_error {
                        ui.colored_label(egui::Color32::RED, format!("Config: {}", err));
                    } else if self.config.is_some() {
                        ui.colored_label(egui::Color32::GREEN, &self.content_path_display);
                    } else {
                        ui.label(&self.content_path_display);
                    }
                });
                ui.separator();

                if ui.button("Scan Stage Directory").clicked() {
                    self.scan_stage_directory();
                }
                if let Some(err) = &self.scan_error {
                    ui.colored_label(egui::Color32::RED, err);
                }

                ui.add_space(5.0);
                ui.label("Found Stage Files (.llm.txt):");
                egui::ScrollArea::vertical()
                    .id_source("stage_files_scroll_gui") // Unique ID
                    .max_height(150.0)
                    .show(ui, |ui| {
                        let mut path_to_load_onclick = None;
                        let files_clone = self.stage_files.clone();
                        for p_idx in 0..files_clone.len() {
                            let p = &files_clone[p_idx];
                            let fname = p.file_name().unwrap_or_default().to_string_lossy();
                            let is_selected = self.selected_stage_file.as_ref() == Some(p);
                            if ui.selectable_label(is_selected, fname).clicked() {
                                if !is_selected {
                                    path_to_load_onclick = Some(p.clone());
                                }
                            }
                        }
                        if let Some(p_clicked) = path_to_load_onclick {
                            self.load_and_parse_selected_file(&p_clicked);
                        }
                    });
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Sentences/Block (GUI Sim):");
                    ui.add(egui::DragValue::new(&mut self.sentences_per_block).speed(1.0).clamp_range(10..=5000));
                });
                ui.horizontal(|ui| {
                    ui.label("Max Sim Passes (GUI Sim):");
                    ui.add(egui::DragValue::new(&mut self.max_simulation_loops).speed(1.0).clamp_range(1..=100));
                });
                ui.separator();

                ui.collapsing("Advanced Simulation Parameters (GUI Sim)", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Max Regen/Block:");
                        ui.add(egui::DragValue::new(&mut self.max_regen_attempts_per_block).speed(1.0).clamp_range(1..=50));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Target CT% (0.5-1.0):");
                        ui.add(egui::DragValue::new(&mut self.target_ct_threshold).speed(0.01).clamp_range(0.50..=1.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Max Activate/Regen:");
                        ui.add(egui::DragValue::new(&mut self.max_words_to_activate_per_regen).speed(1.0).clamp_range(1..=10));
                    });
                });
                ui.separator();

                if self.current_numerical_chapter.is_some() {
                    if ui.button("Run Simulation Orchestrator (GUI)").clicked() {
                        self.run_simulation_orchestrator();
                    }
                } else if self.selected_stage_file.is_some() {
                    ui.label("File selected, but not parsed or error during parsing/conversion.");
                }

                if let Some(err) = &self.generation_error {
                    ui.colored_label(egui::Color32::RED, format!("Runtime Err: {}", err));
                }
                if let Some(err) = &self.parser_display_error {
                    ui.colored_label(egui::Color32::RED, format!("Parser/Load Err: {}", err));
                }
                ui.separator();

                ui.collapsing("Learner Profile Stats (GUI Sim)", |ui| {
                    ui.label(format!("Known Lemmas: {}", self.learner_profile.count_known()));
                    ui.label(format!("Active (only) Lemmas: {}", self.learner_profile.count_active_only()));
                    ui.label(format!("Total Known or Active: {}", self.learner_profile.count_total_known_or_active()));
                    ui.label(format!("Total Vocabulary Size (Global Dict): {}", self.global_lemma_dictionary.size()));
                    ui.label(format!("Profile Vocab Size (Tracked Lemmas): {}", self.learner_profile.vocabulary_size()));
                    ui.label(format!("Sum of all Exposures in Profile: {}", self.learner_profile.total_exposure_count()));
                });
                ui.separator();

                ui.collapsing("Simulation Log (GUI Sim)", |ui| {
                    egui::ScrollArea::vertical()
                        .id_source("sim_log_scroll_gui")
                        .max_height(250.0)
                        .show(ui, |ui| {
                            let mut log_text_display = self.simulation_log_output.clone();
                            ui.add(
                                egui::TextEdit::multiline(&mut log_text_display)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .interactive(false)
                                    .frame(true), // Frame true for better visibility of textedit area
                            );
                        });
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(3, |columns| {
                egui::ScrollArea::both()
                    .id_source("raw_text_scroll_gui_central") // Ensure unique ID
                    .auto_shrink([false, false])
                    .show(&mut columns[0], |ui| {
                        ui.heading("Raw LLM File (.llm.txt)");
                        ui.separator();
                        if self.selected_stage_file.is_some() {
                            let mut s_display = self.selected_file_content.clone();
                            ui.add(
                                egui::TextEdit::multiline(&mut s_display)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .interactive(false)
                                    .frame(true),
                            );
                        } else {
                            ui.label("Select a .llm.txt file from the list.");
                        }
                    });
                egui::ScrollArea::both()
                    .id_source("json_output_scroll_gui_central") // Unique ID
                    .auto_shrink([false, false])
                    .show(&mut columns[1], |ui| {
                        ui.heading("Processed String Chapter (JSON)");
                        ui.separator();
                        if !self.processed_json_output.is_empty() {
                            let mut s_display = self.processed_json_output.clone();
                            ui.add(
                                egui::TextEdit::multiline(&mut s_display)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .interactive(false)
                                    .frame(true),
                            );
                        } else if self.parser_display_error.is_some() {
                            ui.colored_label(egui::Color32::RED, "Parser error (see side panel).");
                        } else if self.selected_stage_file.is_some() && self.current_string_chapter.is_none() {
                            ui.label("File selected, but parsing failed or pending.");
                        } else {
                            ui.label("Parsed string data (JSON view) appears here.");
                        }
                    });
                egui::ScrollArea::both()
                    .id_source("woven_text_scroll_gui_central") // Unique ID
                    .auto_shrink([false, false])
                    .show(&mut columns[2], |ui| {
                        ui.heading("Generated Woven Text (GUI Sim)");
                        ui.separator();
                        if !self.woven_text_output.is_empty() {
                            let mut s_display = self.woven_text_output.clone();
                            ui.add(
                                egui::TextEdit::multiline(&mut s_display)
                                    .desired_width(f32::INFINITY)
                                    .frame(true)
                                    .font(egui::TextStyle::Body), // Normal font for output
                            );
                        } else if self.generation_error.is_some() {
                            let mut s_display = self.generation_error.as_ref().unwrap_or(&String::new()).clone();
                            ui.add(
                                egui::TextEdit::multiline(&mut s_display)
                                    .font(egui::TextStyle::Monospace)
                                    .text_color(egui::Color32::RED)
                                    .interactive(false)
                                    .frame(true),
                            );
                        } else if self.current_numerical_chapter.is_some() {
                            ui.label("Click 'Run Simulation Orchestrator (GUI)'.");
                        } else {
                            ui.label("Load a chapter and then run simulation.");
                        }
                    });
            });
        });
    }
}

// --- Main Function ---
fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    let project_app_config_result = weavelang_rust_gui::config::load_config_from_file(
        cli.config.to_str().unwrap_or("config.toml"),
    );

    let mut project_app_config_for_gui: Option<Config> = None;
    let mut config_error_msg_for_gui: Option<String> = None;

    let config_for_generate_mode: Option<Config>;


    match project_app_config_result {
        Ok(loaded_config) => {
            println!("Successfully loaded project configuration from: {:?}", cli.config);
            project_app_config_for_gui = Some(loaded_config.clone()); // Clone for GUI
            config_for_generate_mode = Some(loaded_config); // Move for generate mode
        }
        Err(err_msg) => {
            eprintln!("Error loading project configuration from {:?}: {}", cli.config, err_msg);
            config_error_msg_for_gui = Some(err_msg.clone());
            project_app_config_for_gui = None;
            config_for_generate_mode = None; // No config available for generate mode
            if matches!(cli.command, Some(Commands::Generate(_))) {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Failed to load config file {:?}: {}", cli.config, err_msg),
                )));
            }
        }
    }
    
    match cli.command.unwrap_or(Commands::Gui) {
        Commands::Gui => {
            println!("Launching GUI mode...");
            let options = NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_inner_size([1600.0, 900.0])
                    .with_min_inner_size([1000.0, 700.0]),
                ..Default::default()
            };
            
            eframe::run_native(
                "WeaveLang Tool - Interactive Mode",
                options,
                Box::new(move |cc| Box::new(WeaveLangApp::new(cc, project_app_config_for_gui, config_error_msg_for_gui))),
            )?;
        }
        Commands::Generate(generate_args) => {
            println!("Starting Corpus Generation mode...");
            // ... (print args as before) ...

            let final_config_for_generate = config_for_generate_mode.ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::Other, "Project config is required for generate mode but was not loaded successfully.")
            })?;

            let corpus_gen_args = corpus_generator::GenerationArgs {
                sequence_path: generate_args.sequence,
                tts_output_dir: generate_args.tts_output_dir,
                profiles_dir: generate_args.profiles_dir,
                start_profile_path: generate_args.start_profile,
                sentences_per_block: generate_args.sentences_per_block,
                max_regen_attempts_per_block: generate_args.max_regen_attempts_per_block,
                target_ct_threshold: generate_args.target_ct_threshold,
                max_words_to_activate_per_regen: generate_args.max_words_to_activate_per_regen,
            };

            if let Err(e) = corpus_generator::run_corpus_generation(&final_config_for_generate, &corpus_gen_args) {
                eprintln!("Corpus generation failed: {}", e);
                std::process::exit(1);
            } else {
                println!("Corpus generation completed successfully.");
            }
        }
    }
    Ok(())
}
//*** END FILE: src/main.rs ***//