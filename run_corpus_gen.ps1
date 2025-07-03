# run_corpus_gen.ps1
# MODIFIED for the new V2 Curriculum-Based Generation Model.

Write-Host "Starting WeaveLang Corpus Generation (V2 Model)..."

# --- Step 1: Compile the Rust project ---
Write-Host "Compiling the Rust project in release mode..."
cargo build --release

# Check if the compilation was successful
if ($LASTEXITCODE -ne 0) {
    Write-Error "Rust compilation failed. Halting script."
    Read-Host -Prompt "Press Enter to exit"
    exit 1
}
Write-Host "Compilation successful."
Write-Host "---"


# --- Step 2: Define paths and arguments ---
# These arguments now define the DEFAULT state for a batch run.
# The sequence.txt file can override 'start-level' and 'ramp-rate' for specific books.

$ExecutablePath = ".\target\release\weavelang_rust_gui.exe"

# IMPORTANT: Please verify this path. The Python pipeline now outputs to 'pipeline/stage8/'.
# This should point to the directory inside your content project that contains the final .json files.
$InputJsonSubDir = "library" 

# Base path for your content project (e.g., 'audiolingual')
$ContentProjectPath = "E:/Bill/Documents/development/audiolingual"

$CommandArgs = @(
    "generate", # The subcommand to run
    "--sequence", "sequence.txt",
    "--input-json-dir", $InputJsonSubDir,
    "--tts-output-dir", "$ContentProjectPath/generated_tts_input",
    #"--debug-markers",
    "--profiles-dir", "$ContentProjectPath/generated_profiles",

    # --- NEW V2 CURRICULUM ARGUMENTS ---
    
    # Default starting level for the very first book in the sequence.
    # Will be overridden by `%level` commands in sequence.txt.
    "--start-level", "0", 
    
    # Default ramp rate. Will be overridden by `%ramp` commands in sequence.txt.
    # A rate of 10 means ~10 new words per hour of content.
    "--ramp-rate", "10", 

    # The number of words that defines a single level.
    "--words-per-level", "10",

    # The number of words from the top of the frequency list that get a slower,
    # tapering introduction rate.
    "--core-vocab-size", "2000",

    # The percentage of progress into a new level required to attempt 'stretching'
    # to complete that level. (0.5 = 50%)
    "--stretch-threshold", "0.5",

    # The maximum allowed acceleration (e.g., 0.15 for 15%) when stretching.
    # Prevents very short books from having an absurd ramp rate.
    "--max-compression-ratio", "0.15"

    # --- OPTIONAL DEBUGGING ---
    # Uncomment the line below to force all sentences to be generated at the Advanced Spanish level.
    #"--force-level", "as"
)

# --- Step 3: Run the compiled executable ---
Write-Host "Running the compiled executable:"
Write-Host "$ExecutablePath $($CommandArgs -join ' ')"
Write-Host "--- RUST APP OUTPUT STARTS HERE ---"

& $ExecutablePath $CommandArgs

$RustExitCode = $LASTEXITCODE
Write-Host "--- RUST APP OUTPUT ENDS HERE ---"
Write-Host "Rust executable finished with exit code: $RustExitCode"

if ($RustExitCode -ne 0) {
    Write-Error "The Rust generation process exited with an error. Please review the output above."
} else {
    Write-Host "Corpus generation command finished successfully."
}

# Pause at the end to see output.
Read-Host -Prompt "Press Enter to exit"