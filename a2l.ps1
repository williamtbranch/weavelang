<#
.SYNOPSIS
    Launches the WeaveLang LLM processing pipeline orchestrator.
.DESCRIPTION
    This script runs orchestrate_pipeline.py as a module, which manages the
    entire multi-stage, depth-first processing of books based on a TOML config.
.NOTES
    Version: 5.0.0 (Refactored to be config-driven)
    Author: Bill Branch
#>

# --- Script Configuration ---
$PythonExecutable = "python" # Or "python3", or full path to python.exe/venv python
$ModulePath = "llm2books.orchestrate_pipeline" 

# --- Execution Control ---
# These are the ONLY arguments passed to the orchestrator now.
# All other settings are in config.toml.
$BookToProcess = $null    # e.g., "AW". Set to $null to process all books.
$ForceBook = $null         # e.g., "AW". Set to $null for normal resumability.
$StartAtStage = $null     # e.g., 3. Set to $null or remove line to start from 1.

# --- Build the command arguments for orchestrate_pipeline.py ---
$CmdArgs = New-Object System.Collections.ArrayList

# --- Add arguments for the orchestrator ---
# The orchestrator will now read all other settings from this file.
[void]$CmdArgs.Add("--project_config"); [void]$CmdArgs.Add("config.toml")
[void]$CmdArgs.Add("--version"); [void]$CmdArgs.Add("7.0.0-config-driven") # Example version

if ($BookToProcess) {
    [void]$CmdArgs.Add("--book_to_process"); [void]$CmdArgs.Add($BookToProcess)
}
if ($ForceBook) {
    [void]$CmdArgs.Add("--force_book"); [void]$CmdArgs.Add($ForceBook)
}
if ($StartAtStage) {
    [void]$CmdArgs.Add("--start_at_stage"); [void]$CmdArgs.Add($StartAtStage.ToString())
}

# --- Execute the Python script ---
Write-Host "Running WeaveLang Pipeline Orchestrator (Config-Driven)..."
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
    Write-Warning "Python orchestrator exited with an error code. Check the pipeline log and stage-specific .log files for details."
}