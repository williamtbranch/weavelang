<#
.SYNOPSIS
    Wrapper script to run stage2llm.py with predefined or passed parameters.
.DESCRIPTION
    This script simplifies running the Python LLM processing script by setting
    default parameters. It's configured by default to use the Claude LLM provider.
    API keys should be set in a .env file or as environment variables:
    GOOGLE_API_KEY for Gemini
    ANTHROPIC_API_KEY for Claude
.NOTES
    Version: 1.1.0 (Adapted for synchronous stage2llm.py and multi-call pipeline)
    Author: Bill Branch
#>

# --- Script Configuration ---
$PythonExecutable = "python" # Or "python3", or full path to python.exe/venv python
# Script name changed from stage2llm_async.py to stage2llm.py
$ScriptPath = Join-Path $PSScriptRoot "stage2llm.py" # Assumes script is in the same dir

# --- LLM Provider and Model Configuration ---
$LLMProvider = "claude" # "gemini" or "claude" (Note: stage2llm.py currently has Gemini SDK example)
                        # You will need to adapt stage2llm.py's make_llm_api_call for Claude if using Claude

# Gemini Model (if $LLMProvider is "gemini")
$GeminiModelName = "models/gemini-1.5-flash-latest" 

# Claude Model (if $LLMProvider is "claude")
# $ClaudeModelName = "claude-3-5-sonnet-20240620" # Claude 3.5 Sonnet (Recommended for balance)
$ClaudeModelName = "claude-3-haiku-20240307"   # Using the specified Haiku model
# $ClaudeModelName = "claude-3-opus-20240229"    # Opus for highest quality
$ClaudeMaxTokens = 4096 # Max output tokens for Claude (Haiku and Sonnet generally support up to 4096 output)

# --- API Key Configuration (CLI args override .env/env vars if stage2llm.py supports them directly) ---
# The new stage2llm.py uses argparse, so API keys passed here will be picked up if defined in its parser.
# $GeminiAPIKey = "your_gemini_key_here_if_not_in_env" 
# $AnthropicAPIKey = "your_anthropic_key_here_if_not_in_env"

# --- File and Directory Paths ---
$ProjectConfig = "config.toml" # Relative to where this script is run, or use absolute path
$InputStagedSubdir = "Staged"  # For bookname.txt files
$OutputLLMSubdir = "stage"     # For final .llm.txt files

# --- Processing Control for stage2llm.py ---
$ForceProcessing = $false      # $true to force reprocess, $false to resume (resume logic in stage2llm.py is basic)
$BatchSize = 10                # Number of sentences per batch for each LLM call stage
$MaxApiRetriesPerBatch = 3     # Retries for a failing batch API call
$RetryDelaySeconds = 7         # Delay between retries for a batch

# $LimitItems is NOT a direct argument for the new stage2llm.py in the same way.
# The new script processes whole books. If limiting is needed, it's usually per book for testing specific parts.
# For now, removing $LimitItems as a direct pass-through argument.
# If you need to process only N sentences from a book, that logic would be inside stage2llm.py or by preparing a smaller input Staged file.

# --- Build the command arguments for stage2llm.py ---
$CmdArgs = New-Object System.Collections.ArrayList

# LLM Model configuration (stage2llm.py currently uses a single --llm_model argument)
# The Python script will need to be adapted if you want to specify different models for Gemini/Claude
# or select provider logic within the Python script. For now, pass the chosen one.
if ($LLMProvider -eq "gemini") {
    [void]$CmdArgs.Add("--llm_model")
    [void]$CmdArgs.Add($GeminiModelName)
    if ($PSBoundParameters.ContainsKey('GeminiAPIKey') -and $GeminiAPIKey) {
        [void]$CmdArgs.Add("--api_key"); [void]$CmdArgs.Add($GeminiAPIKey)
    }
}
elseif ($LLMProvider -eq "claude") {
    [void]$CmdArgs.Add("--llm_model") # stage2llm.py needs to know which model string to use
    [void]$CmdArgs.Add($ClaudeModelName)
    if ($PSBoundParameters.ContainsKey('AnthropicAPIKey') -and $AnthropicAPIKey) {
        [void]$CmdArgs.Add("--api_key"); [void]$CmdArgs.Add($AnthropicAPIKey)
    }
    # Note: stage2llm.py's make_llm_api_call needs to be updated to handle Claude SDK
    # and use $ClaudeMaxTokens. This argument is not directly in the Python script's argparser yet.
    # You might need to add a --claude_max_tokens arg to stage2llm.py if it's specific.
    # For now, this PowerShell var isn't passed.
}


# Add paths
[void]$CmdArgs.Add("--project_config");          [void]$CmdArgs.Add($ProjectConfig)
[void]$CmdArgs.Add("--input_staged_subdir");    [void]$CmdArgs.Add($InputStagedSubdir)
[void]$CmdArgs.Add("--output_llm_subdir");      [void]$CmdArgs.Add($OutputLLMSubdir)

# Add processing controls
if ($ForceProcessing) {
    [void]$CmdArgs.Add("--force")
}
[void]$CmdArgs.Add("--batch_size");              [void]$CmdArgs.Add($BatchSize.ToString())
[void]$CmdArgs.Add("--max_api_retries");         [void]$CmdArgs.Add($MaxApiRetriesPerBatch.ToString())
[void]$CmdArgs.Add("--retry_delay");             [void]$CmdArgs.Add($RetryDelaySeconds.ToString())


# --- Execute the Python script ---
Write-Host "Running stage2llm.py with parameters for LLM Provider: $LLMProvider"
$DebugCmdArgsString = $CmdArgs -join ' '
Write-Host "Command: $PythonExecutable `"$ScriptPath`" $DebugCmdArgsString"
Write-Host "---"

# Execute the command
& $PythonExecutable "$ScriptPath" $CmdArgs

$ExitCode = $LASTEXITCODE
Write-Host "---"
Write-Host "Python script execution finished with exit code: $ExitCode"

if ($ExitCode -ne 0) {
    Write-Warning "Python script exited with an error code."
}