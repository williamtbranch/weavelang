# run_corpus_gen.ps1
# MODIFIED for the new V10 curriculum generation model.

Write-Host "Starting WeaveLang Corpus Generation (V10 Model)..."

# --- Step 1: Run the Rust Test Suite ---
Write-Host "--- Running Rust Test Suite (cargo test) ---" -ForegroundColor Yellow
cargo test
if ($LASTEXITCODE -ne 0) {
    Write-Error "Rust tests failed. Halting script. Please fix the tests before generating the corpus."
    Read-Host -Prompt "Press Enter to exit"
    exit 1
}
Write-Host "--- Rust tests passed. Proceeding. ---" -ForegroundColor Green
Write-Host ""

# --- Step 2: Compile the Rust project ---
Write-Host "Compiling the Rust project in release mode..."
cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Error "Rust compilation failed. Halting script."
    Read-Host -Prompt "Press Enter to exit"
    exit 1
}
Write-Host "Compilation successful."
Write-Host "---"

# --- Step 3: Define paths and arguments ---
$ExecutablePath = ".\target\release\weavelang_rust_gui.exe"
$ContentProjectPath = "E:/Bill/Documents/development/audiolingual"
$ToolRootDir = $PSScriptRoot

$CommandArgs = @(
    "generate",
    "--tool-root-dir", $ToolRootDir,
    "--sequence", "sequence.txt",
    "--input-json-dir", "library",
    "--tts-output-dir", "$ContentProjectPath/generated_tts_input",
    "--profiles-dir", "$ContentProjectPath/generated_profiles",
    "--inverse-diglot-threshold", "0.4"
    #"--debug-markers" # Uncomment for debugging
)

# --- Step 4: Run the compiled executable ---
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

Read-Host -Prompt "Press Enter to exit"