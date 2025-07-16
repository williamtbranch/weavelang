<#
.SYNOPSIS
    A high-level wrapper to run the WeaveLang data generation pipeline with a testing guardrail.
.DESCRIPTION
    This script first runs the entire Python test suite using pytest.
    If and only if all tests pass, it then proceeds to execute the main
    pipeline orchestrator (a2l.ps1). If any test fails, the script halts.
.NOTES
    Version: 1.0.0
    Author: Bill Branch
#>

# --- Step 1: Run the Python Test Suite ---
Write-Host "--- Running Python Test Suite (pytest) ---" -ForegroundColor Yellow

pytest

# Capture the exit code from the test runner
$TestExitCode = $LASTEXITCODE

# --- Step 2: Check Test Results ---
if ($TestExitCode -ne 0) {
    Write-Error "One or more Python tests failed. Halting pipeline execution."
    Write-Error "Please fix the failing tests before generating data."
    
    # Optional: Pause to make sure the user sees the error.
    Read-Host -Prompt "Press Enter to exit"
    exit 1 # Exit with a non-zero code to indicate failure
}

# --- Step 3: Run the Pipeline (only if tests passed) ---
Write-Host ""
Write-Host "--- Python tests passed. Proceeding with pipeline execution. ---" -ForegroundColor Green

# Call the original pipeline script
.\a2l.ps1

$PipelineExitCode = $LASTEXITCODE
Write-Host "--- Pipeline execution finished with exit code: $PipelineExitCode ---"

if ($PipelineExitCode -ne 0) {
    Write-Warning "The pipeline orchestrator exited with an error. Check the logs."
}