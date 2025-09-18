// In src/main.rs

use clap::{Parser}; // <-- Removed 'ValueEnum'
use std::path::PathBuf;
use weavelang_rust_gui::{
    config, corpus_generator,
    simulation::{avd_hunter, calibrator},
    // We no longer need to import CalibrationMode here
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Parser, Debug)]
enum Commands {
    Generate(GenerateCliArgs),
    Hunt(HuntCliArgs),
    Calibrate(CalibrateCliArgs),
}

// --- THIS IS THE UPDATED STRUCT ---
#[derive(clap::Args, Debug, Clone)]
struct CalibrateCliArgs {
    // The --mode and --l-level-data-path arguments have been removed.
    
    #[arg(long, value_name = "FILE", help = "Path to the book's JSON file to calibrate.")]
    book_json: PathBuf,

    #[arg(long, value_name = "FILE", help = "Path for the final output file (e.g., BookName_u_level_map.json).")]
    output_path: PathBuf,

    #[arg(long, default_value_t = 40, help = "The maximum user/l-level to calibrate for.")]
    max_level: u32,
}
// --- END OF UPDATED STRUCT ---

#[derive(clap::Args, Debug, Clone)]
struct HuntCliArgs {
    #[arg(long, value_name = "FILE", help = "Path to the canonical JSON file to run the hunt against.")]
    canonical_json: PathBuf,
    #[arg(long, default_value_t = 50, help = "The maximum number of user levels to discover.")]
    max_user_levels: u32,
    #[arg(long, value_name = "FILE", help = "Path to save the final master_avd_scale.csv file.")]
    output_csv: PathBuf,
}

#[derive(clap::Args, Debug, Clone)]
struct GenerateCliArgs {
    #[arg(long, value_name = "DIR", help="Path to the tool's root directory, for finding assets.")]
    tool_root_dir: PathBuf,
    #[arg(short, long, value_name = "FILE", help="Path to the sequence.txt file listing books to process.")]
    sequence: PathBuf,
    #[arg(long, value_name = "DIR", help="Directory containing the final JSON files (relative to content_project_dir).")]
    input_json_dir: PathBuf,
    #[arg(long, value_name = "DIR", help="Directory to save the final generated TTS text files.")]
    tts_output_dir: PathBuf,
    #[arg(long, value_name = "DIR", help="Directory to save output profiles and analysis logs.")]
    profiles_dir: PathBuf,
    #[arg(long, default_value_t = 0.4, help="Max ratio of substitutions to total words in an L0 sentence to be considered a valid inverse diglot.")]
    inverse_diglot_threshold: f32,
    #[arg(long, help = "Add (%%...%%) markers to the output for debugging.")]
    debug_markers: bool,
}


fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    let config_path = "config.toml";

    let tool_root_dir = match &cli.command {
        Commands::Generate(args) => Some(args.tool_root_dir.clone()),
        Commands::Hunt(_) | Commands::Calibrate(_) => Some(std::env::current_dir()?),
    };

    if let Some(root_dir) = tool_root_dir {
         let freq_list_path = root_dir
            .join("assets")
            .join("frequency_lists")
            .join("es_master_frequency_list.txt");
        weavelang_rust_gui::simulation::frequency_manager::load_master_frequency_list(&freq_list_path)?;
    } else {
        return Err("Could not determine tool root directory to load assets.".into());
    }


    match cli.command {
        Commands::Generate(args) => {
            let project_config = config::load_config_from_file(config_path)?;
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
                eprintln!("[ERROR] Corpus generation failed: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Hunt(args) => {
            if let Err(e) = avd_hunter::run_hunt(
                &args.canonical_json,
                args.max_user_levels,
                &args.output_csv,
            ) {
                eprintln!("[ERROR] AVD Hunter failed: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Calibrate(args) => {
            // --- THIS IS THE UPDATED CALL ---
             if let Err(e) = calibrator::run_unified_calibration(
                &args.book_json,
                args.max_level,
                &args.output_path,
             ) {
                 eprintln!("[ERROR] Book calibration failed: {}", e);
                 std::process::exit(1);
             }
        }
    }
    Ok(())
}