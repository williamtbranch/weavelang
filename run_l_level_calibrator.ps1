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
#$BookStem = "test"

$ExecutablePath = ".\target\release\weavelang_rust_gui.exe"
$ContentProjectPath = "E:/Bill/Documents/development/audiolingual"

# The book we are calibrating. This file MUST exist.
$BookJsonFile = "$ContentProjectPath/library/$($BookStem).json"

# --- START: MODIFIED SECTION ---
# Change: Define the path to the master AVD scale file.
$MasterAvdScaleFile = "$ContentProjectPath/generated_profiles/master_avd_scale.csv"
# Change: The `output-path` is now the FINAL destination for the book file.
# The original file will be read and then overwritten with the new data.
$FinalOutputPath = "$ContentProjectPath/library/$($BookStem).json"

# Change: Define a path for the NEW debug file which will contain the detailed analysis.
$DebugAnalysisFile = "$ContentProjectPath/generated_profiles/$($BookStem)_calibration_data.json"

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
# --- START: MODIFIED SECTION ---
# Change: Add a sanity check for the new required file.
if (-not (Test-Path $MasterAvdScaleFile)) {
    Write-Error "Master AVD Scale file not found at '$MasterAvdScaleFile'."
    Write-Error "Please run the AVD Hunter (`run_avd_hunter.ps1`) first to generate this file."
    Read-Host -Prompt "Press Enter to exit"
    exit 1
}
# --- END: MODIFIED SECTION ---
$CommandArgs = @(
    "calibrate",
    "--book-json", $BookJsonFile,
    "--max-level", $MaxLevelToCalibrate,
    "--output-path", $FinalOutputPath,
    "--master-avd-scale", $MasterAvdScaleFile,
    "--output-debug-path", $DebugAnalysisFile
)

# --- START: MODIFIED SECTION ---
# Change: Add the new `--master-avd-scale` argument to the command.

# --- END: MODIFIED SECTION ---

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
    Write-Host "Book calibration command finished successfully."
    # --- START: MODIFIED SECTION ---
    # Change: Update the success messages to point to the new files.
    Write-Host "  -> The final curriculum map has been merged into: $FinalOutputPath"
    Write-Host "  -> Detailed analysis data has been saved to: $DebugAnalysisFile"
    # --- END: MODIFIED SECTION ---
}

Read-Host -Prompt "Press Enter to exit"