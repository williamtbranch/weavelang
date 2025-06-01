# b2a.ps1

Write-Host "Starting Text-to-Speech conversion..."

# --- Configuration ---
$ToolRootPath = "E:/Bill/development/weavelang"
$PythonScriptPath = "$ToolRootPath/book_to_audio.py"
$InputFileName = "Alice_In_Wonderland_inst01_lvl00_lvl00.txt" # Your test file

# --- TTS Model Configuration ---
$TtsModelName = "models/gemini-2.5-pro-preview-tts" # Your new default Pro model

# --- Run Mode ---
[bool]$RepairMode = $false # Set to $true to attempt a repair run
# To run in repair mode from command line: .\b2a.ps1 -RepairMode $true
# Or just change it here for a specific run.

# --- Processing Parameters ---
# These are defaults for a NORMAL run. In REPAIR mode, some of these might be
# overridden by metadata (chunk_max_chars, tts_prompt_prefix, model_name, voice_name).
$VoiceName = "Schedar"
$OutputAudioFormat = "wav"
$LogLevel = "INFO"       # Use DEBUG for detailed troubleshooting, INFO for normal runs
$ConcurrentRequests = 3  # Adjust based on stability and API limits
$ChunkMaxChars = 1000    
$MaxApiRetries = 10
$RetryDelay = 15          # Base delay in seconds
$TtsPromptPrefix = "You have a Mexican Spanish accent. Narrate the following text in a clear, even voice, suitable for an audiobook at a moderate pace. You are telling a story. Be engaging:"


# --- Construct Command Parameters ---
$PythonParams = @(
    "--input-filename", $InputFileName,  # Enclose in single quotes if names have spaces
    "--tool-root-dir", $ToolRootPath,
    "--model-name", $TtsModelName,
    "--voice-name", $VoiceName,
    "--tts-prompt-prefix", $TtsPromptPrefix, # Important for consistency
    "--output-audio-format", $OutputAudioFormat,
    "--log-level", $LogLevel,
    "--concurrent-requests", $ConcurrentRequests,
    "--chunk-max-chars", $ChunkMaxChars,       # Crucial for consistency
    "--max-api-retries", $MaxApiRetries,
    "--retry-delay", $RetryDelay
)

if ($RepairMode) {
    $PythonParams += "--repair-mode"
    Write-Host "--- REPAIR MODE ENABLED ---"
} else {
    Write-Host "--- NORMAL MODE ---"
}

# --- Display Parameters ---
Write-Host "Executing Python script: $PythonScriptPath"
Write-Host "Input file: $InputFileName"
Write-Host "TTS Model (initial): $TtsModelName" # Python script will log effective model
Write-Host "Voice Name (initial): $VoiceName"
Write-Host "Chunk Max Chars (initial): $ChunkMaxChars"
Write-Host "Max API Retries: $MaxApiRetries"
# ... add other params if desired ...

# --- Run the Command ---
Write-Host "Running: python $PythonScriptPath $($PythonParams -join ' ')"
python $PythonScriptPath $PythonParams

$ExitCode = $LASTEXITCODE
if ($ExitCode -eq 0) {
    Write-Host "Python script finished."
    $ExpectedOutputFile = "E:/Bill/Documents/development/audiolingual/audio/$($InputFileName -replace '\.txt$', ".$OutputAudioFormat")"
    Write-Host "Check for '$ExpectedOutputFile'"
} else {
    Write-Host "Python script exited with error code: $ExitCode"
}

Read-Host -Prompt "Press Enter to exit"