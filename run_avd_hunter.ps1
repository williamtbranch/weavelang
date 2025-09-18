<#
.SYNOPSIS
    Launches the WeaveLang AVD Hunter to generate the Master AVD Scale.
.DESCRIPTION
    This script first runs the Rust test suite to ensure engine correctness.
    If tests pass, it compiles the project in release mode and then invokes
    the 'hunt' command on the Rust executable. The hunt process analyzes a

    canonical JSON file to discover the relationship between vocabulary size,
    new lemma density, and the AVD score, outputting the results to a CSV.
.NOTES
    Version: 1.0.0
    Author: Bill Branch
#>

# --- Step 1: Run the Rust Test Suite ---
Write-Host "--- Running Rust Test Suite (cargo test) ---" -ForegroundColor Yellow
cargo test

# Check if the tests were successful
if ($LASTEXITCODE -ne 0) {
    Write-Error "Rust tests failed. Halting AVD Hunt. Please fix the tests before proceeding."
    Read-Host -Prompt "Press Enter to exit"
    exit 1
}
Write-Host "--- Rust tests passed. Proceeding. ---" -ForegroundColor Green
Write-Host ""


# --- Step 2: Compile the Rust project ---
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


# --- Step 3: Define paths and arguments for the AVD Hunter ---

$ExecutablePath = ".\target\release\weavelang_rust_gui.exe"
$ContentProjectPath = "E:/Bill/Documents/development/audiolingual"

# This is the large, representative JSON file the hunter will analyze.
# Ensure you have run the Python pipeline on a suitable book (e.g., your combined test book).
$CanonicalJsonFile = "$ContentProjectPath/library/LesMis.json"

# The final output file containing the discovered user levels.
$OutputCsvFile = "$ContentProjectPath/generated_profiles/master_avd_scale.csv"

# The number of user levels to discover. Start with a smaller number for testing.
$MaxUserLevels = 60

# --- Sanity Check ---
if (-not (Test-Path $CanonicalJsonFile)) {
    Write-Error "Canonical JSON file not found at '$CanonicalJsonFile'."
    Write-Error "Please ensure the path is correct and you have run the Python pipeline on this book."
    Read-Host -Prompt "Press Enter to exit"
    exit 1
}

$CommandArgs = @(
    "hunt",
    "--canonical-json", $CanonicalJsonFile,
    "--max-user-levels", $MaxUserLevels,
    "--output-csv", $OutputCsvFile
)

# --- Step 4: Run the compiled executable ---
Write-Host "Starting the AVD Hunter..."
Write-Host "Running command:"
Write-Host "$ExecutablePath $($CommandArgs -join ' ')"
Write-Host "--- RUST AVD HUNTER OUTPUT STARTS HERE ---"

& $ExecutablePath $CommandArgs

$RustExitCode = $LASTEXITCODE
Write-Host "--- RUST AVD HUNTER OUTPUT ENDS HERE ---"
Write-Host "AVD Hunter finished with exit code: $RustExitCode"

if ($RustExitCode -ne 0) {
    Write-Error "The AVD Hunter process exited with an error. Please review the output above."
} else {
    Write-Host "AVD Hunt command finished successfully."
    Write-Host "Check for the generated scale at: $OutputCsvFile"
}

Read-Host -Prompt "Press Enter to exit"