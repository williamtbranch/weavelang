<#
.SYNOPSIS
    Launches the WeaveLang LLM processing pipeline orchestrator.
.DESCRIPTION
    This script runs orchestrate_pipeline.py, which manages the entire
    multi-stage, depth-first processing of books.
.NOTES
    Version: 3.3.1 (Re-added StartAtStage)
    Author: Bill Branch
#>

# --- Script Configuration ---
$PythonExecutable = "python" # Or "python3", or full path to python.exe/venv python
$ScriptPath = Join-Path $PSScriptRoot "llm2books\orchestrate_pipeline.py" 

# --- LLM Provider and Model Configuration ---
$LLMProvider = "claude" # "gemini" or "claude"
$ClaudeModelName = "claude-3-5-haiku-20241022"
#$ClaudeModelName = "claude-3-haiku-20240307"
#$ClaudeModelName = "claude-3-7-sonnet-20250219"
$ClaudeFallbackModelName = "claude-3-7-sonnet-20250219" # More capable model for difficult cases

# --- Batching Configuration ---
$MaxSentencesPerBatch = 5     # Max sentences per batch (for most stages)
$MaxBatchTokens = 2000        # Not currently used by the worker, but good to keep for future use

# --- Execution Control ---
$BookToProcess = $null  # e.g., "AW"
#$ForceBook = "AW"       # e.g., "AW"
$StartAtStage = 1      # e.g., 3. Set to $null or remove line to start from 1.

# --- Pass-through arguments for the worker script ---
$MaxValidationRetries = 4

# --- Build the command arguments for orchestrate_pipeline.py ---
$CmdArgs = New-Object System.Collections.ArrayList

# --- Add arguments for the orchestrator ---
[void]$CmdArgs.Add("--project_config"); [void]$CmdArgs.Add("config.toml")

if ($BookToProcess) {
    [void]$CmdArgs.Add("--book_to_process"); [void]$CmdArgs.Add($BookToProcess)
}
if ($ForceBook) {
    [void]$CmdArgs.Add("--force_book"); [void]$CmdArgs.Add($ForceBook)
}
if ($StartAtStage) { # <<< ADDED THIS BLOCK
    [void]$CmdArgs.Add("--start_at_stage"); [void]$CmdArgs.Add($StartAtStage.ToString())
}
    
# --- Add pass-through arguments for the worker ---
[void]$CmdArgs.Add("--llm_provider"); [void]$CmdArgs.Add($LLMProvider)
[void]$CmdArgs.Add("--llm_model"); [void]$CmdArgs.Add($ClaudeModelName)
[void]$CmdArgs.Add("--llm_fallback_model"); [void]$CmdArgs.Add($ClaudeFallbackModelName) 

[void]$CmdArgs.Add("--max_sentences_per_batch"); [void]$CmdArgs.Add($MaxSentencesPerBatch.ToString())
[void]$CmdArgs.Add("--max_validation_retries"); [void]$CmdArgs.Add($MaxValidationRetries.ToString())

# --- Execute the Python script ---
Write-Host "Running WeaveLang Pipeline Orchestrator..."
$DebugCmdArgsString = $CmdArgs -join ' '
Write-Host "Command: $PythonExecutable `"$ScriptPath`" $DebugCmdArgsString"
Write-Host "---"

# Execute the command
& $PythonExecutable "$ScriptPath" $CmdArgs

$ExitCode = $LASTEXITCODE
Write-Host "---"
Write-Host "Python script execution finished with exit code: $ExitCode"

if ($ExitCode -ne 0) {
    Write-Warning "Python orchestrator exited with an error code. Check pipeline_orchestrator.log and pipeline_orchestrator.err for details."
}