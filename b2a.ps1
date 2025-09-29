# b2a.ps1 (Updated for Gemini 2.5 Pro Support)

Write-Host "Starting Text-to-Speech conversion..."

# --- Configuration ---
$ToolRootPath = "E:/Bill/development/weavelang" # Your actual path
$PythonScriptPath = "$ToolRootPath/book_to_audio.py"
$InputFileName = "Ozma_of_Oz03_inst01.txt" # Your test file

# --- TTS Service Selection ---
$TtsService = "vertex" # "gemini" or "vertex". Select "gemini" to use the new 2.5 Pro voices.
# $TtsService = "vertex" 

# --- Gemini TTS Configuration (used if $TtsService is "gemini") ---
# New Gemini 2.5 Pro and Flash models are supported.
$GeminiModelName = "models/gemini-2.5-pro-preview-tts"  # <<< CHANGED: New default Pro model
# $GeminiModelName = "models/gemini-2.5-flash-preview-tts" # Alternative Flash model
# $GeminiModelName = "models/gemini-1.5-flash-preview-tts" # Older model

# The voice names (e.g., Schedar, Puck, Kore) are compatible with the new models.
$GeminiVoiceName = "Schedar" # Default Gemini voice
$GeminiTtsPromptPrefix = "You are a professional voice actor with a Mexican Spanish accent."

# --- Vertex AI TTS Configuration (used if $TtsService is "vertex") ---
#$VertexVoiceName = "es-US-Chirp3-HD-Achernar" # Example Vertex voice, find more at Google Cloud docs
#$VertexVoiceName = "es-US-Chirp3-HD-Aoede"
$VertexVoiceName = "es-US-Chirp3-HD-Achernar"
#$VertexVoiceName = "es-US-Chirp3-HD-Algieba" # Example Vertex voice, find more at Google Cloud docs
$VertexLanguageCode = "es-US"      # Example, e.g., "es-MX"

# --- Run Mode ---
[bool]$RepairMode = $false # Set to $true to attempt a repair run

# --- Common Processing Parameters ---
# In REPAIR mode, some of these might be overridden by metadata from the previous run.
$OutputAudioFormat = "wav"
$LogLevel = "INFO"       # DEBUG, INFO, WARNING, ERROR, CRITICAL
$ConcurrentRequests = 1  # Start with 1, increase cautiously
$ChunkMaxChars = 3000    
$MaxApiRetries = 4 
$RetryDelay = 15          # Base delay in seconds

# --- Construct Command Parameters ---
$PythonParams = @(
    "--input-filename", $InputFileName,
    "--tool-root-dir", $ToolRootPath,
    "--tts-service", $TtsService,
    "--output-audio-format", $OutputAudioFormat,
    "--log-level", $LogLevel,
    "--concurrent-requests", $ConcurrentRequests,
    "--chunk-max-chars", $ChunkMaxChars,
    "--max-api-retries", $MaxApiRetries,
    "--retry-delay", $RetryDelay
)

# Add service-specific parameters
if ($TtsService -eq "gemini") {
    $PythonParams += "--model-name", $GeminiModelName
    $PythonParams += "--voice-name", $GeminiVoiceName
    $PythonParams += "--tts-prompt-prefix", $GeminiTtsPromptPrefix
    # API Key for Gemini can be in .env or passed via --api-key if needed
    # $PythonParams += "--api-key", "YOUR_GEMINI_API_KEY_IF_NOT_IN_ENV" 
    Write-Host "Using GEMINI TTS service."
    Write-Host "Gemini Model (initial): $GeminiModelName"
    Write-Host "Gemini Voice (initial): $GeminiVoiceName"
} elseif ($TtsService -eq "vertex") {
    $PythonParams += "--voice-name", $VertexVoiceName
    $PythonParams += "--language-code", $VertexLanguageCode
    Write-Host "Using VERTEX AI TTS service. Ensure ADC is configured ('gcloud auth application-default login')."
    Write-Host "Vertex Voice (initial): $VertexVoiceName"
    Write-Host "Vertex Language Code: $VertexLanguageCode"
}

if ($RepairMode) {
    $PythonParams += "--repair-mode"
    Write-Host "--- REPAIR MODE ENABLED ---"
} else {
    Write-Host "--- NORMAL MODE ---"
}

# --- Display Common Parameters ---
Write-Host "Executing Python script: $PythonScriptPath"
Write-Host "Input file: $InputFileName"
Write-Host "Chunk Max Chars (initial): $ChunkMaxChars"
# ... add other common params if desired ...

# --- Run the Command ---
Write-Host "Running: python $PythonScriptPath $($PythonParams -join ' ')"
python $PythonScriptPath $PythonParams

$ExitCode = $LASTEXITCODE
if ($ExitCode -eq 0) {
    Write-Host "Python script finished."
    # Adjust path if your content_project_dir in config.toml is different
    $BaseAudioDir = "E:/Bill/Documents/development/audiolingual/audio" # Example base from your .ps1
    $ExpectedOutputFile = "$BaseAudioDir/$($InputFileName -replace '\.txt$', ".$OutputAudioFormat")"
    Write-Host "Check for '$ExpectedOutputFile'"
} else {
    Write-Host "Python script exited with error code: $ExitCode"
}

Read-Host -Prompt "Press Enter to exit"