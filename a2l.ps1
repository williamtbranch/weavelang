<#
.SYNOPSIS
    Launches the WeaveLang data generation pipeline (V9 - Common Pool Architecture).
.DESCRIPTION
    This script runs orchestrate_pipeline.py, which now uses a PoolManager
    to generate reusable data artifacts and then runs a lean pipeline to
    produce pair-specific data for a single, specified book.
.NOTES
    Version: 9.0.0 (Refactored for Common Pool Architecture)
    Author: Bill Branch
#>

# --- Script Configuration ---

$PythonExecutable = ".\.venv\Scripts\python.exe"
$ModulePath = "llm2books.orchestrate_pipeline" 
# --- Execution Control ---
# These arguments control a single pipeline run for a specific book and language pair.
#$BookToProcess = "quijote_test"    # The book stem, e.g., "Grimm", "test". This is now MANDATORY.
$BookToProcess = "LesMis"    # The book stem, e.g., "Grimm", "test". This is now MANDATORY.
$BaseLang      = "en"      # The base language for this run.
$TargetLang    = "es"      # The target language for this run.
$StopAfterStage = 0

# --- Sanity Check ---
if (-not $BookToProcess) {
    Write-Error "The `$BookToProcess` variable cannot be empty. Please specify the book stem to process."
    exit 1
}

# --- Build the command arguments for orchestrate_pipeline.py ---
$CmdArgs = New-Object System.Collections.ArrayList

# Add arguments for the orchestrator
[void]$CmdArgs.Add("--project_config"); [void]$CmdArgs.Add("config.toml")
[void]$CmdArgs.Add("--book-to-process"); [void]$CmdArgs.Add($BookToProcess)
[void]$CmdArgs.Add("--base-lang");       [void]$CmdArgs.Add($BaseLang)
[void]$CmdArgs.Add("--target-lang");     [void]$CmdArgs.Add($TargetLang)

if ($StopAfterStage -gt 0) {
    [void]$CmdArgs.Add("--stop-after-stage"); [void]$CmdArgs.Add($StopAfterStage)
}

# --- Execute the Python script ---
Write-Host "Running WeaveLang Pipeline Orchestrator (Common Pool)..."
$DebugCmdArgsString = $CmdArgs -join ' '
Write-Host "Command: $PythonExecutable -m $ModulePath $DebugCmdArgsString"
Write-Host "---"

Push-Location $PSScriptRoot
& $PythonExecutable -m $ModulePath $CmdArgs
$ExitCode = $LASTEXITCODE
Pop-Location

Write-Host "---"
Write-Host "Python script execution finished with exit code: $ExitCode"

if ($ExitCode -ne 0) {
    Write-Warning "Python orchestrator exited with an error code. Check the pipeline log for details."
}