// src/main.rs

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use eframe::NativeOptions;
use std::path::PathBuf;
use weavelang_rust_gui::{
    config, corpus_generator,
    global_settings::GlobalSettings,
    gui::app::WeaveLangApp,
    services::llm_client::LlmService,
    services::llm_logger::LlmLogger,
    services::prompt_manager::PromptManager,
    services::python_bridge::BridgeService,
    simulation::{avd_hunter, calibrator, frequency_manager},
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Parser, Debug)]
enum Commands {
    Gui,
    Generate(GenerateCliArgs),
    Hunt(HuntCliArgs),
    Calibrate(CalibrateCliArgs),
}

#[derive(clap::Args, Debug, Clone)]
struct CalibrateCliArgs {
    #[arg(long, value_name = "FILE")]
    book_json: PathBuf,
    #[arg(long, value_name = "FILE")]
    output_path: PathBuf,
    #[arg(long, value_name = "FILE")]
    output_debug_path: Option<PathBuf>,
    #[arg(long, value_name = "FILE")]
    master_avd_scale: PathBuf,
    #[arg(long, default_value_t = 40)]
    max_level: u32,
}

#[derive(clap::Args, Debug, Clone)]
struct HuntCliArgs {
    #[arg(long, value_name = "FILE")]
    canonical_json: PathBuf,
    #[arg(long, default_value_t = 50)]
    max_user_levels: u32,
    #[arg(long, value_name = "FILE")]
    output_csv: PathBuf,
}

#[derive(clap::Args, Debug, Clone)]
struct GenerateCliArgs {
    #[arg(long, value_name = "DIR")]
    tool_root_dir: PathBuf,
    #[arg(short, long, value_name = "FILE")]
    sequence: PathBuf,
    #[arg(long, value_name = "DIR")]
    input_json_dir: PathBuf,
    #[arg(long, value_name = "DIR")]
    tts_output_dir: PathBuf,
    #[arg(long, value_name = "DIR")]
    profiles_dir: PathBuf,
    #[arg(long, default_value_t = 0.4)]
    inverse_diglot_threshold: f32,
    #[arg(long)]
    debug_markers: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();

    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Commands::Gui);

    match command {
        Commands::Gui => {
            println!("[INFO] Launching GUI...");

            // 1. Try to load config — non-fatal so the GUI starts even without config.toml.
            //    Priority: CWD/config.toml → last workspace from global settings → None.
            let initial_config = {
                let cwd_config = config::load_config_from_file("config.toml").ok();
                if cwd_config.is_some() {
                    println!("[INFO] Loaded config.toml from current directory.");
                    cwd_config
                } else {
                    let gs = GlobalSettings::load();
                    if let Some(ws) = &gs.last_workspace {
                        let ws_config_path = std::path::Path::new(ws).join("config.toml");
                        match config::load_config_from_file(ws_config_path.to_str().unwrap_or("")) {
                            Ok(cfg) => {
                                println!("[INFO] Loaded config from last workspace: {ws}");
                                Some(cfg)
                            }
                            Err(e) => {
                                eprintln!("[WARN] Could not load last workspace config: {e}");
                                None
                            }
                        }
                    } else {
                        eprintln!("[INFO] No config.toml found — open a workspace to begin.");
                        None
                    }
                }
            };

            // 2. Logger — only if we have a content directory
            let logger = initial_config.as_ref().map(|cfg| {
                let content_dir = cfg.content_project_dir_path();
                println!("[INFO] Content directory: {content_dir:?}");
                LlmLogger::new(content_dir)
            });

            // 3. Load Assets (Frequency List)
            let freq_list_path = std::env::current_dir()?
                .join("assets/frequency_lists/es_master_frequency_list.txt");
            if freq_list_path.exists() {
                let _ = frequency_manager::load_master_frequency_list(&freq_list_path);
            }

            // 4. Initialize Services
            let bridge = match std::env::current_dir() {
                Ok(cwd) => match BridgeService::new(cwd) {
                    Ok(b) => {
                        println!("[INFO] Python Bridge initialized.");
                        Some(b)
                    }
                    Err(e) => {
                        eprintln!("[WARN] Bridge Error: {e}");
                        None
                    }
                },
                Err(_) => None,
            };

            let llm = {
                let models = initial_config.as_ref()
                    .map(|c| c.models.clone())
                    .unwrap_or_default();
                let thinking_budget = initial_config.as_ref()
                    .and_then(|c| c.pipeline.thinking_budget_tokens);
                let svc = LlmService::new_routing_with_thinking(std::env::current_dir().ok(), models, thinking_budget);
                println!("[INFO] LLM Service initialized (multi-provider routing, thinking_budget: {:?}).", thinking_budget);
                Some(svc)
            };

            let prompts = match std::env::current_dir() {
                Ok(cwd) => Some(PromptManager::new(cwd)),
                Err(_) => None,
            };

            let options = NativeOptions {
                viewport: eframe::egui::ViewportBuilder::default()
                    .with_inner_size([1280.0, 800.0])
                    .with_min_inner_size([800.0, 600.0])
                    .with_title("WeaveLang Studio"),
                ..Default::default()
            };

            eframe::run_native(
                "WeaveLang Studio",
                options,
                Box::new(move |cc| Box::new(WeaveLangApp::new(cc, bridge, llm, prompts, logger, initial_config))),
            )?;
        }

        // ... (Keep existing Generate/Hunt/Calibrate commands unchanged) ...
        Commands::Generate(args) => {
            let freq_list_path = args
                .tool_root_dir
                .join("assets/frequency_lists/es_master_frequency_list.txt");
            frequency_manager::load_master_frequency_list(&freq_list_path)?;
            let project_config = config::load_config_from_file("config.toml")?;
            if let Err(e) = corpus_generator::run_corpus_generation(
                &project_config,
                &args.tool_root_dir,
                &args.sequence,
                &args.input_json_dir,
                &args.tts_output_dir,
                &args.profiles_dir,
                args.debug_markers,
                args.inverse_diglot_threshold,
            ) {
                eprintln!("[ERROR] {e}");
                std::process::exit(1);
            }
        }
        Commands::Hunt(args) => {
            if let Err(e) =
                avd_hunter::run_hunt(&args.canonical_json, args.max_user_levels, &args.output_csv)
            {
                eprintln!("[ERROR] {e}");
                std::process::exit(1);
            }
        }
        Commands::Calibrate(args) => {
            // Quick asset load for calibrate
            let _ = std::env::current_dir().map(|d| {
                frequency_manager::load_master_frequency_list(
                    &d.join("assets/frequency_lists/es_master_frequency_list.txt"),
                )
            });
            if let Err(e) = calibrator::run_unified_calibration(
                &args.book_json,
                &args.output_path,
                args.output_debug_path.as_deref(),
                &args.master_avd_scale,
                args.max_level,
            ) {
                eprintln!("[ERROR] {e}");
                std::process::exit(1);
            }
        }
    }
    Ok(())
}
