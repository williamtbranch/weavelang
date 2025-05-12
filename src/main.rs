#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod parser;
mod profile;
mod generation;

use eframe::{egui, App, NativeOptions};
use std::fs;
use std::path::PathBuf;
use std::collections::HashMap; 

use config::Config;
// Remove ProcessedSentence from this line
use parser::{parse_llm_text_to_chapter, ProcessedChapter}; 
use profile::LearnerProfile; 
// Remove GenerationOutput from this line
use generation::{generate_woven_text_for_chapter}; 

struct WeaveLangApp {
    config: Option<Config>, config_error: Option<String>, content_path_display: String,
    stage_files: Vec<PathBuf>, selected_stage_file: Option<PathBuf>,
    selected_parsed_chapter: Option<ProcessedChapter>, selected_file_content: String,
    scan_error: Option<String>, processed_json_output: String,
    learner_profile: LearnerProfile, woven_text_output: String,
    generation_error: Option<String>, parser_display_error: Option<String>,
    current_ct_info: String,
    sentences_per_block: usize,
    max_simulation_loops: u32,
}

impl WeaveLangApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut config_val = None;
        let mut config_error_val = None;
        let content_path_display_val: String;

        match config::load_config_from_file("config.toml") {
            Ok(loaded_config) => {
                content_path_display_val = format!("Content Dir: {}", loaded_config.content_project_dir);
                config_val = Some(loaded_config);
            }
            Err(err_msg) => {
                eprintln!("Error loading config.toml: {}", err_msg);
                config_error_val = Some(err_msg.clone());
                content_path_display_val = err_msg;
            }
        }

        Self {
            config: config_val,
            config_error: config_error_val,
            content_path_display: content_path_display_val,
            stage_files: Vec::new(),
            selected_stage_file: None,
            selected_parsed_chapter: None,
            selected_file_content: String::new(),
            scan_error: None,
            processed_json_output: String::new(),
            learner_profile: LearnerProfile::new(),
            woven_text_output: String::new(),
            generation_error: None,
            parser_display_error: None,
            current_ct_info: String::new(),
            sentences_per_block: 100, 
            max_simulation_loops: 2000, // Example large value
        }
    }

    fn scan_stage_directory(&mut self) {
        self.stage_files.clear(); self.selected_stage_file = None; self.selected_parsed_chapter = None;
        self.selected_file_content.clear(); self.scan_error = None; self.processed_json_output.clear();
        self.parser_display_error = None; self.woven_text_output.clear(); self.generation_error = None;
        self.current_ct_info.clear();
        if let Some(conf) = &self.config {
            let stage_path = PathBuf::from(&conf.content_project_dir).join("stage");
            if !stage_path.is_dir() { self.scan_error = Some(format!("Stage directory not found: {:?}", stage_path)); return; }
            match fs::read_dir(stage_path) {
                Ok(entries) => { for entry in entries { if let Ok(entry) = entry { let path = entry.path(); if path.is_file() { if let Some(name_str) = path.file_name().and_then(|n| n.to_str()) { if name_str.ends_with(".llm.txt") { self.stage_files.push(path); }}}}} if self.stage_files.is_empty() { self.scan_error = Some("No .llm.txt files found.".to_string()); } self.stage_files.sort(); }
                Err(e) => { self.scan_error = Some(format!("Failed to read stage directory: {}", e)); }
            }
        } else { self.scan_error = Some("Config not loaded.".to_string()); }
    }

    fn load_and_parse_file(&mut self, path: &PathBuf) {
        self.processed_json_output.clear(); self.parser_display_error = None; self.selected_parsed_chapter = None;
        self.woven_text_output.clear(); self.generation_error = None; self.current_ct_info.clear();
        match fs::read_to_string(path) {
            Ok(contents) => { 
                self.selected_file_content = contents.clone(); self.selected_stage_file = Some(path.clone()); self.scan_error = None;
                let file_name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                match parse_llm_text_to_chapter(&file_name, &contents) {
                    Ok(parsed_chapter) => { 
                        if !parsed_chapter.sentences.is_empty() {
                            self.sentences_per_block = (parsed_chapter.sentences.len() * 5).max(50); // e.g. 5 chapter reps or 50 sentences min
                            simulation_log_push_for_app(&mut self.current_ct_info, &format!("Adjusted sentences_per_block to {} for this chapter.", self.sentences_per_block));
                        }
                        self.selected_parsed_chapter = Some(parsed_chapter.clone()); 
                        match serde_json::to_string_pretty(&parsed_chapter) { 
                            Ok(json_string) => self.processed_json_output = json_string, 
                            Err(e) => self.parser_display_error = Some(format!("JSON Serialization failed: {}", e)),
                        }
                    }
                    Err(e) => { self.parser_display_error = Some(format!("Parser Error: {}", e)); self.selected_parsed_chapter = None; }
                }
            }
            Err(e) => { self.selected_file_content = format!("Error loading file {:?}: {}", path.file_name().unwrap_or_default(), e); self.selected_stage_file = Some(path.clone()); }
        }
    }
    
    fn generate_woven_text_with_ct_loop(&mut self) {
        self.woven_text_output.clear(); 
        self.generation_error = None;
        self.current_ct_info.clear();

        if let Some(chapter_data_ref) = &self.selected_parsed_chapter {
            let chapter_data = chapter_data_ref.clone(); 
            
            const TARGET_CT_COMPREHENSIBLE_THRESHOLD: f32 = 0.98; 
            const MAX_REGENERATION_ATTEMPTS_PER_BLOCK: u32 = 25;
            const MAX_WORDS_TO_ACTIVATE_PER_REGEN_ATTEMPT: usize = 3;

            let mut current_overall_profile = self.learner_profile.clone(); 
            let mut accumulated_output_for_display: String = String::new(); 
            let mut simulation_log_parts: Vec<String> = Vec::new(); // Changed name to avoid conflict

            let mut source_lemmas_freq: HashMap<String, u32> = HashMap::new();
            for s in &chapter_data.sentences {
                s.adv_s_lemmas.iter().for_each(|l| *source_lemmas_freq.entry(l.clone()).or_insert(0) += 1);
                s.sim_s_lemmas.iter().for_each(|sl| sl.lemmas.iter().for_each(|l| *source_lemmas_freq.entry(l.clone()).or_insert(0) += 1));
                for seg_map in &s.diglot_map {
                    for entry in &seg_map.entries {
                        if entry.viable {
                            *source_lemmas_freq.entry(entry.spa_lemma.clone()).or_insert(0) += 1;
                        }
                    }
                }
            }
            let mut sorted_source_lemmas_for_activation: Vec<(String, u32)> = source_lemmas_freq.into_iter().collect();
            sorted_source_lemmas_for_activation.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

            simulation_log_push(&mut simulation_log_parts, &format!("Starting simulation. Sentences/block: {}. Max sim_loops: {}. Max Regen/block: {}", self.sentences_per_block, self.max_simulation_loops, MAX_REGENERATION_ATTEMPTS_PER_BLOCK));
            let initial_known = current_overall_profile.count_known();
            let initial_active = current_overall_profile.count_active_only();
            let initial_total_ka = current_overall_profile.count_total_known_or_active();
            simulation_log_push(&mut simulation_log_parts, &format!("Initial Profile: Known: {}, Active (only): {}, Total K/A: {}", initial_known, initial_active, initial_total_ka));
            accumulated_output_for_display.push_str(&format!("%%WEAVELANG_STAT%% INITIAL PROFILE: Known: {}, Active (only): {}, Total K/A: {}\n\n", initial_known, initial_active, initial_total_ka));

            'outer_simulation_loop: for sim_loop_num in 1..=self.max_simulation_loops {
                simulation_log_push(&mut simulation_log_parts, &format!("\n--- Simulation Loop: {} (Generating Measurement Block {}) ---", sim_loop_num, sim_loop_num));
                accumulated_output_for_display.push_str(&format!("\n\n%%WEAVELANG_STAT%% START SIMULATION LOOP {} / MEASUREMENT BLOCK {}\n", sim_loop_num, sim_loop_num));
                
                let profile_at_start_of_sim_loop_refinement = current_overall_profile.clone(); // For outer saturation check
                let mut profile_being_refined_for_block = current_overall_profile.clone(); 
                
                let mut finalized_block_text_this_sim_loop: String = String::from("Error: Block text not set by inner loop.");
                let mut finalized_block_lemmas_this_sim_loop: Vec<String> = Vec::new();
                let mut profile_of_finalized_block: LearnerProfile = profile_being_refined_for_block.clone(); 

                for regen_attempt in 1..=MAX_REGENERATION_ATTEMPTS_PER_BLOCK {
                    simulation_log_push(&mut simulation_log_parts, &format!("  Attempting to generate Measurement Block {} (Regen Attempt: {})", sim_loop_num, regen_attempt));
                    
                    let mut profile_for_this_generation_pass = profile_being_refined_for_block.clone();

                    let mut sentences_processed_this_attempt = 0;
                    let mut current_attempt_text_parts: Vec<String> = Vec::new();
                    let mut current_attempt_lemmas_used: Vec<String> = Vec::new(); 
                    let mut chapter_loop_count_this_attempt = 0;

                    while sentences_processed_this_attempt < self.sentences_per_block {
                        chapter_loop_count_this_attempt += 1;
                        if chapter_loop_count_this_attempt > 1 && regen_attempt == 1 {
                             current_attempt_text_parts.push("\n\n%%WEAVELANG_STAT%% --- Chapter Repetition Boundary within Block ---\n\n".to_string());
                        }
                        
                        match generate_woven_text_for_chapter(&chapter_data, &mut profile_for_this_generation_pass) {
                            Ok(gen_output_for_chapter_pass) => {
                                current_attempt_text_parts.push(gen_output_for_chapter_pass.woven_text);
                                current_attempt_lemmas_used.extend(gen_output_for_chapter_pass.spanish_lemmas_used_in_output);
                                sentences_processed_this_attempt += chapter_data.sentences.len(); 
                            }
                            Err(e) => {
                                self.generation_error = Some(format!("Error generating chapter pass for Block {}, Regen {}: {}", sim_loop_num, regen_attempt, e));
                                simulation_log_push(&mut simulation_log_parts, self.generation_error.as_ref().unwrap());
                                accumulated_output_for_display.push_str(&format!("\n%%WEAVELANG_STAT%% ERROR: {}\n", self.generation_error.as_ref().unwrap()));
                                self.current_ct_info = simulation_log_parts.join("\n");
                                self.learner_profile = current_overall_profile; 
                                self.woven_text_output = accumulated_output_for_display;
                                return; 
                            }
                        }
                        if sentences_processed_this_attempt >= self.sentences_per_block { break; }
                    }
                    
                    let current_attempt_text = current_attempt_text_parts.join("");

                    let known_instances_in_attempt = if !current_attempt_lemmas_used.is_empty() {
                        current_attempt_lemmas_used.iter()
                            .filter(|lemma_str| profile_for_this_generation_pass.get_lemma_info(lemma_str).map_or(false, |info| info.state == crate::profile::LemmaState::Known))
                            .count()
                    } else { 0 };
                    
                    let actual_ct_this_attempt = if !current_attempt_lemmas_used.is_empty() {
                        known_instances_in_attempt as f32 / current_attempt_lemmas_used.len() as f32
                    } else { 0.0 };
                    
                    simulation_log_push(&mut simulation_log_parts, &format!("    Block {} Regen Attempt {}: CT={:.2}% ({}K/{}T)", sim_loop_num, regen_attempt, actual_ct_this_attempt * 100.0, known_instances_in_attempt, current_attempt_lemmas_used.len()));

                    let block_is_too_easy = actual_ct_this_attempt >= TARGET_CT_COMPREHENSIBLE_THRESHOLD && !current_attempt_lemmas_used.is_empty();
                    let block_has_no_spanish = current_attempt_lemmas_used.is_empty();
                    let final_regen_attempt = regen_attempt == MAX_REGENERATION_ATTEMPTS_PER_BLOCK;

                    if (!block_is_too_easy && !block_has_no_spanish) || final_regen_attempt { 
                        if final_regen_attempt && (block_is_too_easy || block_has_no_spanish) {
                            simulation_log_push(&mut simulation_log_parts, &format!("    Max regeneration attempts ({}) reached for Block {}. Finalizing with this attempt despite CT/NoSpanish.", MAX_REGENERATION_ATTEMPTS_PER_BLOCK, sim_loop_num));
                            // accumulated_output_for_display.push_str(&format!("%%WEAVELANG_STAT%% MAX REGEN ATTEMPTS FOR BLOCK {}. FINALIZING.\n", sim_loop_num)); // Already logged if conditions met below
                        } else if !block_has_no_spanish { // CT is good
                            simulation_log_push(&mut simulation_log_parts, &format!("    CT for Block {} met ({:.2}%) and contains Spanish. Finalizing this block.", sim_loop_num, actual_ct_this_attempt * 100.0));
                        } else { // No spanish, but not too easy (e.g. first attempt) - this is fine if it's the only option
                             simulation_log_push(&mut simulation_log_parts, &format!("    Block {} has no Spanish, accepting this version.", sim_loop_num));
                        }
                        finalized_block_text_this_sim_loop = current_attempt_text;
                        finalized_block_lemmas_this_sim_loop = current_attempt_lemmas_used;
                        profile_of_finalized_block = profile_for_this_generation_pass; 
                        break; 
                    } else { 
                        if block_has_no_spanish {
                            simulation_log_push(&mut simulation_log_parts, &format!("    Block {} Regen Attempt {} produced no Spanish. Trying to activate new word(s) for next regen.", sim_loop_num, regen_attempt));
                            accumulated_output_for_display.push_str(&format!("%%WEAVELANG_STAT%% BLOCK {}, RegenAttempt {}, NO SPANISH. Activating.\n", sim_loop_num, regen_attempt));
                        } else { 
                            simulation_log_push(&mut simulation_log_parts, &format!("    Block {} Regen Attempt {} CT {:.2}% is too easy. Trying to activate new word(s) for next regen.", sim_loop_num, regen_attempt, actual_ct_this_attempt * 100.0));
                            accumulated_output_for_display.push_str(&format!("%%WEAVELANG_STAT%% BLOCK {}, RegenAttempt {}, CT TOO HIGH ({:.2}%). Activating.\n", sim_loop_num, regen_attempt, actual_ct_this_attempt*100.0));
                        }
                        
                        let mut words_activated_this_regen_cycle = 0;
                        for (lemma_to_activate, freq) in &sorted_source_lemmas_for_activation {
                            if profile_being_refined_for_block.get_lemma_info(lemma_to_activate).map_or(true, |info| info.state == crate::profile::LemmaState::New) {
                                simulation_log_push(&mut simulation_log_parts, &format!("      Activating (for next regen of block {}): '{}' (Freq: {})", sim_loop_num, lemma_to_activate, freq));
                                accumulated_output_for_display.push_str(&format!("%%WEAVELANG_STAT%% ACTIVATING FOR NEXT REGEN: '{}' (SourceFreq: {})\n", lemma_to_activate, freq));
                                let info = profile_being_refined_for_block.get_lemma_info_mut(lemma_to_activate); 
                                info.state = crate::profile::LemmaState::Active;
                                words_activated_this_regen_cycle += 1;
                                if words_activated_this_regen_cycle >= MAX_WORDS_TO_ACTIVATE_PER_REGEN_ATTEMPT {
                                    break;
                                }
                            }
                        }
                        if words_activated_this_regen_cycle == 0 {
                            simulation_log_push(&mut simulation_log_parts, &format!("    Block {} (Regen Attempt {}) condition for activation met, but no more 'New' words to activate. Finalizing with current attempt.", sim_loop_num, regen_attempt));
                            accumulated_output_for_display.push_str(&format!("%%WEAVELANG_STAT%% BLOCK {}, RegenAttempt {}, NO NEW WORDS TO ACTIVATE FOR REGEN. FINALIZING.\n", sim_loop_num, regen_attempt));
                            finalized_block_text_this_sim_loop = current_attempt_text;
                            finalized_block_lemmas_this_sim_loop = current_attempt_lemmas_used;
                            profile_of_finalized_block = profile_for_this_generation_pass; 
                            break; 
                        }
                    }
                } 

                accumulated_output_for_display.push_str(&finalized_block_text_this_sim_loop);
                current_overall_profile = profile_of_finalized_block;

                let final_known_in_block = if !finalized_block_lemmas_this_sim_loop.is_empty() {
                     finalized_block_lemmas_this_sim_loop.iter().filter(|lemma_str| current_overall_profile.get_lemma_info(lemma_str).map_or(false, |info| info.state == crate::profile::LemmaState::Known)).count()
                } else {0};
                let final_ct_for_block = if !finalized_block_lemmas_this_sim_loop.is_empty() {
                    final_known_in_block as f32 / finalized_block_lemmas_this_sim_loop.len() as f32
                } else {0.0};

                accumulated_output_for_display.push_str(&format!(
                    "\n\n%%WEAVELANG_STAT%% FINALIZED MEASUREMENT BLOCK {}: CT={:.2}%, KnownInBlock={}, TotalSpanishInBlock={}\n", 
                    sim_loop_num, final_ct_for_block * 100.0, final_known_in_block, finalized_block_lemmas_this_sim_loop.len()
                ));
                
                let known_at_sim_loop_end = current_overall_profile.count_known();
                let active_at_sim_loop_end = current_overall_profile.count_active_only();
                let total_ka_at_sim_loop_end = current_overall_profile.count_total_known_or_active();
                let vocab_size_at_sim_loop_end = current_overall_profile.vocabulary_size();
                let exposures_at_sim_loop_end = current_overall_profile.total_exposure_count();

                simulation_log_push(&mut simulation_log_parts, &format!("  Profile after finalizing block {}: Known: {}, Active (only): {}, Total K/A: {}, Vocab Size: {}, Total Exposures: {}", 
                    sim_loop_num, known_at_sim_loop_end, active_at_sim_loop_end, total_ka_at_sim_loop_end, vocab_size_at_sim_loop_end, exposures_at_sim_loop_end
                ));
                accumulated_output_for_display.push_str(&format!(
                    "%%WEAVELANG_STAT%% PROFILE AFTER SIM_LOOP {}: Known={}, ActiveOnly={}, TotalK/A={}, VocabSize={}, TotalExposures={}\n", 
                    sim_loop_num, known_at_sim_loop_end, active_at_sim_loop_end, total_ka_at_sim_loop_end, vocab_size_at_sim_loop_end, exposures_at_sim_loop_end
                ));

                if current_overall_profile.vocabulary == profile_at_start_of_sim_loop_refinement.vocabulary && sim_loop_num > 1 {
                     simulation_log_push(&mut simulation_log_parts, &format!("  Overall profile vocabulary unchanged after Measurement Block {}. Global saturation likely. Stopping simulation.", sim_loop_num));
                     accumulated_output_for_display.push_str(&format!("%%WEAVELANG_STAT%% GLOBAL SATURATION LIKELY AFTER BLOCK {}. PROFILE VOCABULARY UNCHANGED.\n\n", sim_loop_num));
                     break 'outer_simulation_loop;
                }

                if sim_loop_num < self.max_simulation_loops {
                     accumulated_output_for_display.push_str(&format!(
                        "\n\n%%WEAVELANG_STAT%% >>>>>>>>>> END OF SIM_LOOP {} / STARTING SIM_LOOP {} >>>>>>>>>>\n\n",
                        sim_loop_num, sim_loop_num + 1
                    ));
                } else {
                    accumulated_output_for_display.push_str(&format!("\n\n%%WEAVELANG_STAT%% >>>>>>>>>> END OF MAX SIMULATION LOOPS ({}) >>>>>>>>>>\n\n", sim_loop_num));
                }
            } 

            self.woven_text_output = accumulated_output_for_display; 
            self.learner_profile = current_overall_profile; 
            self.current_ct_info = simulation_log_parts.join("\n");

        } else {
            self.generation_error = Some("No parsed chapter data available.".to_string());
        }
    } // end generate_woven_text_with_ct_loop
} // end impl WeaveLangApp

// Helper function for logging to avoid repeating self.current_ct_info.push_str(...)
fn simulation_log_push(log_parts: &mut Vec<String>, message: &str) {
    log_parts.push(message.to_string());
}
// Helper for logging directly to the app's display string from load_and_parse (if needed, not used currently)
#[allow(dead_code)]
fn simulation_log_push_for_app(log_string: &mut String, message: &str) {
    log_string.push_str(message);
    log_string.push('\n');
}


impl App for WeaveLangApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| { if ui.button("Exit").clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }});
                ui.menu_button("Profile", |ui| { if ui.button("Reset Learner Profile").clicked() {
                    self.learner_profile = LearnerProfile::new();
                    self.woven_text_output.clear();
                    self.generation_error = None;
                    self.current_ct_info.clear();
                }});
            });
        });
        egui::SidePanel::left("side_panel_left").min_width(220.0).default_width(300.0).show(ctx, |ui| { 
            ui.heading("Controls & Info"); ui.separator();
            ui.collapsing("Configuration", |ui| { if let Some(err) = &self.config_error { ui.colored_label(egui::Color32::RED, format!("Config: {}", err)); } else if self.config.is_some() { ui.colored_label(egui::Color32::GREEN, &self.content_path_display); } else { ui.label(&self.content_path_display); }});
            ui.separator();
            if ui.button("Scan Stage Directory").clicked() { self.scan_stage_directory(); }
            if let Some(err) = &self.scan_error { ui.colored_label(egui::Color32::RED, err); }
            ui.add_space(5.0);
            ui.label("Found Stage Files (.llm.txt):");
            egui::ScrollArea::vertical().id_source("stage_files_scroll").max_height(100.0).show(ui, |ui| {
                let mut path_to_load = None;
                for p_idx in 0..self.stage_files.len() { let p = &self.stage_files[p_idx]; let fname = p.file_name().unwrap_or_default().to_string_lossy(); let sel = self.selected_stage_file.as_ref() == Some(p); if ui.selectable_label(sel, fname).clicked() && !sel { path_to_load = Some(p.clone()); }}
                if let Some(p) = path_to_load { self.load_and_parse_file(&p); }
            });
            ui.separator();
            ui.label(format!("Sentences per measurement block: {}", self.sentences_per_block));
            ui.add(egui::Slider::new(&mut self.sentences_per_block, 10..=1000).text("Sentences/Block")); // Allow adjustment
            ui.label(format!("Max simulation loops: {}", self.max_simulation_loops));
            ui.add(egui::Slider::new(&mut self.max_simulation_loops, 1..=5000).text("Max Sim Loops")); // Allow adjustment
            ui.separator();
            if self.selected_parsed_chapter.is_some() { if ui.button("Run Simulation").clicked() { self.generate_woven_text_with_ct_loop(); }}
            else if self.selected_stage_file.is_some() { ui.label("File selected, but not parsed or error."); }
            
            if let Some(err) = &self.generation_error { ui.colored_label(egui::Color32::RED, format!("Gen Err: {}", err)); }
            if let Some(err) = &self.parser_display_error { ui.colored_label(egui::Color32::RED, format!("Parser Err: {}",err)); }
            ui.separator();
            
            ui.collapsing("Learner Profile Stats (End of Simulation)", |ui| { 
                ui.label(format!("Known Lemmas: {}", self.learner_profile.count_known()));
                ui.label(format!("Active (only) Lemmas: {}", self.learner_profile.count_active_only()));
                ui.label(format!("Total Known or Active: {}", self.learner_profile.count_total_known_or_active()));
                ui.label(format!("Total Vocabulary Size: {}", self.learner_profile.vocabulary_size()));
                ui.label(format!("Sum of all Exposures: {}", self.learner_profile.total_exposure_count()));
            });
            ui.separator();
            
            ui.collapsing("Simulation Log", |ui| {egui::ScrollArea::vertical().id_source("ct_log_scroll").max_height(200.0).show(ui, |ui| {
                let mut log_text = self.current_ct_info.clone();
                ui.add(egui::TextEdit::multiline(&mut log_text).font(egui::TextStyle::Monospace).interactive(false).frame(false));
            });});
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(3, |columns| {
                egui::ScrollArea::both().id_source("raw_text_scroll").auto_shrink([false,false]).show(&mut columns[0], |ui| { 
                    ui.heading("Raw LLM File (.llm.txt)"); 
                    ui.separator(); 
                    if self.selected_stage_file.is_some() { 
                        let mut s = self.selected_file_content.clone(); 
                        ui.add(egui::TextEdit::multiline(&mut s)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .frame(true)); 
                    } else { 
                        ui.label("Select a .llm.txt file."); 
                    }});
                egui::ScrollArea::both().id_source("json_output_scroll").auto_shrink([false,false]).show(&mut columns[1], |ui| { 
                    ui.heading("Processed JSON"); 
                    ui.separator(); 
                    if !self.processed_json_output.is_empty() { 
                        let mut s = self.processed_json_output.clone(); 
                        ui.add(egui::TextEdit::multiline(&mut s)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .frame(true)); 
                    } else if self.parser_display_error.is_some() { 
                        ui.label("Parser error (see side panel)."); 
                    } else if self.selected_parsed_chapter.is_none() && self.selected_stage_file.is_some() { 
                        ui.label("File selected, not parsed or error."); 
                    } else { 
                        ui.label("Parsed data appears here."); 
                    }});
                egui::ScrollArea::both().id_source("woven_text_scroll").auto_shrink([false,false]).show(&mut columns[2], |ui| { 
                    ui.heading("Generated Woven Text (Accumulated)"); 
                    ui.separator(); 
                    if !self.woven_text_output.is_empty() { 
                        let mut s = self.woven_text_output.clone(); 
                        ui.add(egui::TextEdit::multiline(&mut s)
                            .desired_width(f32::INFINITY)
                            .frame(true)); 
                    } else if self.generation_error.is_some() { 
                        let mut s = self.generation_error.as_ref().unwrap_or(&String::new()).clone(); 
                        ui.add(egui::TextEdit::multiline(&mut s)
                            .font(egui::TextStyle::Monospace)
                            .text_color(egui::Color32::RED)
                            .interactive(false) 
                            .frame(true)); 
                    } else { 
                        ui.label("Click 'Run Simulation'."); 
                    }
                });
            });
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([1600.0, 900.0]).with_min_inner_size([800.0, 600.0]), ..Default::default() };
    eframe::run_native("Weavelang Tool Prototype (Rust)", options, Box::new(|cc| Box::new(WeaveLangApp::new(cc))))
}