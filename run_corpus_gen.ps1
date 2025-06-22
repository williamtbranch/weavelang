# run_corpus_gen.ps1
# MODIFIED to compile and then run the executable directly for better debugging.

Write-Host "Starting WeaveLang Corpus Generation (from JSON)..."

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
$ExecutablePath = ".\target\release\weavelang_rust_gui.exe"
$CommandArgs = @(
    "generate",
    "--sequence", "sequence.txt",
    "--input-json-dir", "library",
    "--tts-output-dir", "E:/Bill/Documents/development/audiolingual/generated_tts_input",
    "--profiles-dir", "E:/Bill/Documents/development/audiolingual/generated_profiles",
    "--sentences-per-block", "200",
    #"--max-regen-attempts-per-block", "5",
    "--max-words-to-add-per-block", "50", 
    "--target-ct-threshold", "0.97",
    #"--max-words-to-activate-per-regen", "2",
    "--words-per-level", "20"
)

# --- Step 3: Run the compiled executable directly ---
Write-Host "Running the compiled executable directly:"
Write-Host "$ExecutablePath $($CommandArgs -join ' ')"
Write-Host "--- RUST APP OUTPUT STARTS HERE ---"

& $ExecutablePath $CommandArgs

$RustExitCode = $LASTEXITCODE
Write-Host "--- RUST APP OUTPUT ENDS HERE ---"
Write-Host "Rust executable finished with exit code: $RustExitCode"


Write-Host "Corpus generation command finished."
# Pause at the end to see output if running from explorer (optional)
Read-Host -Prompt "Press Enter to exit"