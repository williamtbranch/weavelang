
// src/main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // Keep for release builds

// --- Standard Library Imports ---
use std::path::PathBuf;
use std::fs;

// --- External Crate Imports ---
use clap::Parser;
use eframe::{egui, App as EframeApp, NativeOptions};

// --- Crate-Specific Imports (from our library `weavelang_rust_gui`) ---
use weavelang_rust_gui::{
    config,
    Config,
    GenerationArgs,
    run_corpus_generation,
    JsonChapter,
    parse_chapter_from_json,
};

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
    #[arg(long, value_name = "DIR")]
    input_json_dir: PathBuf,
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

// --- GUI Application (Unchanged) ---
struct WeaveLangApp {
    config: Option<Config>,
    config_error: Option<String>,
    content_path_display: String,
    json_files: Vec<PathBuf>,
    selected_json_file: Option<PathBuf>,
    selected_file_raw_content: String,
    current_json_chapter: Option<JsonChapter>,
    parse_error_display: Option<String>,
}
impl WeaveLangApp {
    fn new(
        _cc: &eframe::CreationContext<'_>,
        app_config: Option<Config>,
        config_error_msg: Option<String>,
    ) -> Self {
        let content_path_display_val = match &app_config {
            Some(conf) => format!("Content Dir: {}", conf.content_project_dir),
            None => config_error_msg.clone().unwrap_or_else(|| "Config load error.".to_string()),
        };
        Self {
            config: app_config,
            config_error: config_error_msg,
            content_path_display: content_path_display_val,
            json_files: Vec::new(),
            selected_json_file: None,
            selected_file_raw_content: String::new(),
            current_json_chapter: None,
            parse_error_display: None,
        }
    }
    fn reset_selected_file_data(&mut self) {
        self.selected_file_raw_content.clear();
        self.current_json_chapter = None;
        self.parse_error_display = None;
    }
    fn scan_final_json_directory_gui(&mut self) {
        self.json_files.clear();
        self.selected_json_file = None;
        self.parse_error_display = None;
        self.reset_selected_file_data();
        if let Some(conf) = &self.config {
            let json_path = PathBuf::from(&conf.content_project_dir).join("stage/stage7");
            if !json_path.is_dir() {
                self.parse_error_display = Some(format!("Final JSON directory not found: {:?}", json_path));
                return;
            }
            match fs::read_dir(json_path) {
                Ok(entries) => {
                    for entry in entries.filter_map(Result::ok) {
                        let path = entry.path();
                        if path.is_file() && path.extension().map_or(false, |ext| ext == "json") {
                            self.json_files.push(path);
                        }
                    }
                    if self.json_files.is_empty() {
                        self.parse_error_display = Some("No .json files found in stage/stage7 directory.".to_string());
                    }
                    self.json_files.sort();
                }
                Err(e) => { self.parse_error_display = Some(format!("Failed to read JSON directory: {}", e)); }
            }
        } else { self.parse_error_display = Some("Config not loaded, cannot scan.".to_string()); }
    }
    fn load_selected_json_file_gui(&mut self, path_to_load: &PathBuf) {
        self.reset_selected_file_data();
        self.selected_json_file = Some(path_to_load.clone());
        match fs::read_to_string(path_to_load) {
            Ok(contents) => {
                self.selected_file_raw_content = contents.clone();
                match parse_chapter_from_json(&contents) {
                    Ok(parsed_chapter) => { self.current_json_chapter = Some(parsed_chapter); }
                    Err(e) => { self.parse_error_display = Some(format!("Parse error: {}", e)); }
                }
            }
            Err(e) => {
                self.parse_error_display = Some(format!("Error loading file {:?}: {}", path_to_load.file_name().unwrap_or_default(), e));
            }
        }
    }
}
impl EframeApp for WeaveLangApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel_gui").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Exit").clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
                });
            });
        });
        egui::SidePanel::left("side_panel_left_gui")
            .min_width(250.0)
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.heading("WeaveLang Tool GUI");
                ui.separator();
                ui.collapsing("Configuration", |ui| {
                    if let Some(err) = &self.config_error {
                        ui.colored_label(egui::Color32::RED, format!("Config Error: {}", err));
                    } else {
                        ui.label(&self.content_path_display);
                    }
                });
                ui.separator();
                if ui.button("Scan for Final .json files").clicked() {
                    self.scan_final_json_directory_gui();
                }
                if let Some(err) = &self.parse_error_display {
                    ui.colored_label(egui::Color32::RED, err);
                }
                ui.add_space(5.0);
                ui.label("Found .json Files:");
                egui::ScrollArea::vertical().id_source("json_files_scroll_gui").max_height(200.0).show(ui, |ui| {
                    let mut path_to_load_onclick = None;
                    for p in &self.json_files {
                        let fname = p.file_name().unwrap_or_default().to_string_lossy();
                        let is_selected = self.selected_json_file.as_ref() == Some(p);
                        if ui.selectable_label(is_selected, fname).clicked() {
                            if !is_selected { path_to_load_onclick = Some(p.clone()); }
                        }
                    }
                    if let Some(p_clicked) = path_to_load_onclick {
                        self.load_selected_json_file_gui(&p_clicked);
                    }
                });
                ui.separator();
                ui.label("GUI Simulation (Currently Placeholder):");
                ui.label("Full GUI simulation functionality pending update for new data pipeline.");
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("JSON File Content Viewer");
            ui.separator();
            if self.selected_json_file.is_some() {
                let mut raw_content_display = self.selected_file_raw_content.clone();
                egui::ScrollArea::both().id_source("raw_json_content_scroll_gui").show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut raw_content_display)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .interactive(false)
                            .frame(true),
                    );
                });
            } else {
                ui.label("Select a .json file from the list to view its raw content.");
            }
        });
    }
}


// --- Main Function (MODIFIED WITH DEBUG PRINTS) ---
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("[RUST_DEBUG] main() started."); // <-- DEBUG PRINT 1
    
    let cli = Cli::parse();
    println!("[RUST_DEBUG] CLI arguments parsed: {:?}", cli); // <-- DEBUG PRINT 2

    let project_app_config_result = config::load_config_from_file(
        cli.config.to_str().unwrap_or("config.toml"),
    );
    println!("[RUST_DEBUG] Config load attempt finished."); // <-- DEBUG PRINT 3

    let (app_config_for_gui, config_error_msg_for_gui, config_for_generate_mode) = 
        match project_app_config_result {
            Ok(loaded_config) => {
                println!("[RUST_DEBUG] Config loaded successfully: {:?}", loaded_config); // <-- DEBUG PRINT 4
                (Some(loaded_config.clone()), None, Some(loaded_config))
            }
            Err(err_msg) => {
                eprintln!("[RUST_DEBUG] Error loading config: {}", err_msg); // <-- DEBUG PRINT 5 (Error case)
                if matches!(cli.command, Some(Commands::Generate(_))) {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Config load failed for generate mode: {}", err_msg),
                    )));
                }
                (None, Some(err_msg), None)
            }
    };
    
    // Use unwrap_or(Commands::Gui) to handle the case where no command is given
    match cli.command.unwrap_or(Commands::Gui) {
        Commands::Gui => {
            println!("[RUST_DEBUG] Launching GUI mode...");
            let options = NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_inner_size([1200.0, 800.0])
                    .with_min_inner_size([800.0, 600.0]),
                ..Default::default()
            };
            
            eframe::run_native(
                "WeaveLang Tool",
                options,
                Box::new(move |cc| Box::new(WeaveLangApp::new(cc, app_config_for_gui, config_error_msg_for_gui))),
            )?;
        }
        Commands::Generate(generate_cli_args) => {
            println!("[RUST_DEBUG] Entering Generate command logic..."); // <-- DEBUG PRINT 6
            println!("[RUST_DEBUG]   Generate CLI Args: {:?}", generate_cli_args);

            let final_config_for_generate = config_for_generate_mode.ok_or_else(|| {
                "Project config is required for generate mode but was not available.".to_string() 
            })?;
            println!("[RUST_DEBUG]   Config for generate mode confirmed.");

            let corpus_gen_args = GenerationArgs {
                sequence_path: generate_cli_args.sequence,
                input_json_dir: generate_cli_args.input_json_dir,
                tts_output_dir: generate_cli_args.tts_output_dir,
                profiles_dir: generate_cli_args.profiles_dir,
                start_profile_path: generate_cli_args.start_profile,
                sentences_per_block: generate_cli_args.sentences_per_block,
                max_regen_attempts_per_block: generate_cli_args.max_regen_attempts_per_block,
                target_ct_threshold: generate_cli_args.target_ct_threshold,
                max_words_to_activate_per_regen: generate_cli_args.max_words_to_activate_per_regen,
            };
            println!("[RUST_DEBUG]   GenerationArgs struct created.");
            println!("[RUST_DEBUG]   Calling run_corpus_generation...");

            if let Err(e) = run_corpus_generation(&final_config_for_generate, &corpus_gen_args) {
                eprintln!("[RUST_DEBUG] run_corpus_generation returned an error: {}", e);
                std::process::exit(1);
            } else {
                println!("[RUST_DEBUG] run_corpus_generation completed without error.");
            }
        }
    }
    println!("[RUST_DEBUG] main() finished successfully.");
    Ok(())
}