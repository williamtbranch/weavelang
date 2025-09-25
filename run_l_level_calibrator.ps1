<#
.SYNOPSIS
    Launches Stage A of the WeaveLang Book Calibration process.
.DESCRIPTION
    This script runs the L-Level Hunter for a specific book.
    
    First, it runs the Rust test suite to ensure engine correctness. If tests
    pass, it compiles the project in release mode.
    
    Then, it invokes the 'calibrate' command, which performs an independent
    AVD hunt for each of the four tiers (Simple, Basic, Moderate, Advanced).
    It discovers the V-Level required to hit the target AVD score for each
    fractional L-Level and saves the complete tables to a temporary JSON file
    for analysis.
.NOTES
    Version: 1.0.0
    Author: Bill Branch
#>

# --- Step 1: Run the Rust Test Suite ---
Write-Host "--- Running Rust Test Suite (cargo test) ---" -ForegroundColor Yellow
cargo test

if ($LASTEXITCODE -ne 0) {
    Write-Error "Rust tests failed. Halting Calibration. Please fix the tests before proceeding."
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


# --- Step 3: Define paths and arguments for the L-Level Calibrator ---
$BookStem = "Metamorphosis"

$ExecutablePath = ".\target\release\weavelang_rust_gui.exe"
$ContentProjectPath = "E:/Bill/Documents/development/audiolingual"

# The book we are calibrating. This file MUST exist.
$BookJsonFile = "$ContentProjectPath/library/$($BookStem).json"

# The INPUT for this run is the output from Stage A.
#$LLevelDataFile = "$ContentProjectPath/generated_profiles/LesMis_l_level_data.json"

# The FINAL output file containing the U-Level map.
$FinalULevelMapFile = "$ContentProjectPath/generated_profiles/$($BookStem)_u_level_map.json"

# The temporary output file for our L-Level tables.
$TempOutputFile = "$ContentProjectPath/generated_profiles/LesMis_l_level_data.json"

# The maximum User/L-Level to calibrate up to.
# Start with a small number (e.g., 5-10) for a quick test run.
# A full run to level 40 will take a significant amount of time.
$MaxLevelToCalibrate = 45 

# --- Sanity Check ---
if (-not (Test-Path $BookJsonFile)) {
    Write-Error "Book JSON file not found at '$BookJsonFile'."
    Write-Error "Please ensure the path is correct and you have run the Python pipeline on this book."
    Read-Host -Prompt "Press Enter to exit"
    exit 1
}

$CommandArgs = @(
    "calibrate",
    "--book-json", $BookJsonFile,
    "--max-level", $MaxLevelToCalibrate,
    "--output-path", $FinalULevelMapFile
)
# $CommandArgs = @(
#      "calibrate",
#     "--mode", "l-level", # This is the default, but good to be clear
#     "--book-json", $BookJsonFile,
#     "--max-level", $MaxLevelToCalibrate,
#     "--output-path", $TempOutputFile
# )

# --- Step 4: Run the compiled executable ---
Write-Host "Starting the L-Level Calibrator (Stage A)..."
Write-Host "Running command:"
Write-Host "$ExecutablePath $($CommandArgs -join ' ')"
Write-Host "--- RUST CALIBRATOR OUTPUT STARTS HERE ---"

& $ExecutablePath $CommandArgs

$RustExitCode = $LASTEXITCODE
Write-Host "--- RUST CALIBRATOR OUTPUT ENDS HERE ---"
Write-Host "Calibrator finished with exit code: $RustExitCode"

if ($RustExitCode -ne 0) {
    Write-Error "The Calibrator process exited with an error. Please review the output above."
} else {
    Write-Host "L-Level calibration command finished successfully."
    Write-Host "Check for the generated data at: $TempOutputFile"
}

Read-Host -Prompt "Press Enter to exit"