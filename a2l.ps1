<#
.SYNOPSIS
    Launches the WeaveLang LLM processing pipeline orchestrator.
.DESCRIPTION
    This script runs orchestrate_pipeline.py as a module, which manages the
    entire multi-stage, depth-first processing of books.
.NOTES
    Version: 4.0.0 (Refactored to use class-based stages and run as a module)
    Author: Bill Branch
#>

# --- Script Configuration ---
$PythonExecutable = "python" # Or "python3", or full path to python.exe/venv python
# NEW: Define the entry point as a Python module path, not a file path.
$ModulePath = "llm2books.orchestrate_pipeline" 

# --- LLM Provider and Model Configuration ---
$LLMProvider = "claude" # "gemini" or "claude"
$ClaudeModelName = "claude-sonnet-4-20250514"
#$ClaudeFallbackModelName = "claude-3-7-sonnet-20250219" # More capable model for difficult cases
$ClaudeFallbackModelName = "claude-opus-4-20250514" # More capable model for difficult cases

# --- Batching Configuration ---
$MaxSentencesPerBatch = 20     # Max sentences per batch (for most stages)
#$MaxBatchTokens = 2000        # Not currently used by the worker, but good to keep for future use

# --- Execution Control ---
#$BookToProcess = "AW"    # e.g., "AW". Set to $null to process all books.
#$ForceBook = "AW"         # e.g., "AW". Set to $null for normal resumability.
$StartAtStage = $null        # e.g., 3. Set to $null or remove line to start from 1.

# --- Pass-through arguments for the worker script ---
$MaxValidationRetries = 4
$MaxApiRetries = 3
$RetryDelay = 5

# --- Build the command arguments for orchestrate_pipeline.py ---
$CmdArgs = New-Object System.Collections.ArrayList

# --- Add arguments for the orchestrator ---
[void]$CmdArgs.Add("--project_config"); [void]$CmdArgs.Add("config.toml")
[void]$CmdArgs.Add("--version"); [void]$CmdArgs.Add("6.0.0-class-based") # Example version

if ($BookToProcess) {
    [void]$CmdArgs.Add("--book_to_process"); [void]$CmdArgs.Add($BookToProcess)
}
if ($ForceBook) {
    [void]$CmdArgs.Add("--force_book"); [void]$CmdArgs.Add($ForceBook)
}
if ($StartAtStage) {
    [void]$CmdArgs.Add("--start_at_stage"); [void]$CmdArgs.Add($StartAtStage.ToString())
}
    
# --- Add pass-through arguments for the worker ---
[void]$CmdArgs.Add("--llm_provider"); [void]$CmdArgs.Add($LLMProvider)
[void]$CmdArgs.Add("--llm_model"); [void]$CmdArgs.Add($ClaudeModelName)
[void]$CmdArgs.Add("--llm_fallback_model"); [void]$CmdArgs.Add($ClaudeFallbackModelName) 

[void]$CmdArgs.Add("--max_sentences_per_batch"); [void]$CmdArgs.Add($MaxSentencesPerBatch.ToString())
[void]$CmdArgs.Add("--max_api_retries"); [void]$CmdArgs.Add($MaxApiRetries.ToString())
[void]$CmdArgs.Add("--max_validation_retries"); [void]$CmdArgs.Add($MaxValidationRetries.ToString())
[void]$CmdArgs.Add("--retry_delay"); [void]$CmdArgs.Add($RetryDelay.ToString())

# --- Execute the Python script ---
Write-Host "Running WeaveLang Pipeline Orchestrator..."
$DebugCmdArgsString = $CmdArgs -join ' '
# NEW: The command format now uses "-m" and the module path
Write-Host "Command: $PythonExecutable -m $ModulePath $DebugCmdArgsString"
Write-Host "---"

# NEW: The execution logic is changed to use the -m flag.
# We ensure we are running from the project root, where this .ps1 file lives.
Push-Location $PSScriptRoot
& $PythonExecutable -m $ModulePath $CmdArgs
$ExitCode = $LASTEXITCODE
Pop-Location

Write-Host "---"
Write-Host "Python script execution finished with exit code: $ExitCode"

if ($ExitCode -ne 0) {
    Write-Warning "Python orchestrator exited with an error code. Check pipeline_orchestrator.log and pipeline_orchestrator.err for details."
}