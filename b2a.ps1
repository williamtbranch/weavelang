# b2a.ps1 (Corrected for Multiple Voice Argument Passing)

# --- START OF HELPER FUNCTION ---
# (This function is unchanged)
function Import-EnvFile {
    param (
        [string]$Path = ".env"
    )
    if (Test-Path $Path) {
        Get-Content $Path | ForEach-Object {
            $line = $_.Trim()
            if ($line -and $line -notmatch "^\s*#") {
                $parts = $line -split '=', 2
                if ($parts.Length -eq 2) {
                    $name = $parts[0].Trim()
                    $value = $parts[1].Trim()
                    if (($value.StartsWith('"') -and $value.EndsWith('"')) -or ($value.StartsWith("'") -and $value.EndsWith("'"))) {
                        $value = $value.Substring(1, $value.Length - 2)
                    }
                    [System.Environment]::SetEnvironmentVariable($name, $value, "Process")
                }
            }
        }
    }
}
# --- END OF HELPER FUNCTION ---


Write-Host "Starting Text-to-Speech conversion..."

# --- Load Environment Variables ---
Import-EnvFile -Path (Join-Path $PSScriptRoot ".env")


# --- Configuration ---
$ToolRootPath = "E:/Bill/development/weavelang" # Your actual path
$PythonScriptPath = "$ToolRootPath/book_to_audio.py"
$InputFileName = "Metamorphosis_UL27.txt" # Your test file

# --- TTS Service Selection ---
$TtsService = "gemini" 
[bool]$UseVertexAuthForGemini = $true 

# --- Gemini TTS Configuration ---
$GeminiModelName = "models/gemini-2.5-pro-preview-tts"
$GeminiVoiceName = @("Charon", "aoede", "Puck", "Zephyr", "Fenrir", "Kore", "Orus", "Leda") 
$GeminiTtsPromptPrefix = "You are a professional voice actor with a Mexican Spanish accent. Your are narrating a Spanglish text. Make sure to read the English as English and Spanish as Spanish."

# --- Vertex AI TTS Configuration ---
$VertexVoiceName = "es-US-Chirp3-HD-Achernar"
$VertexLanguageCode = "es-US"

# --- Run Mode ---
[bool]$RepairMode = $true

# --- Common Processing Parameters ---
$OutputAudioFormat = "wav"
$LogLevel = "INFO"
$ConcurrentRequests = 1
$ChunkMaxChars = 5000
$MaxApiRetries = 2
$RetryDelay = 20
$DelayBetweenChunks = 0

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
    "--retry-delay", $RetryDelay,
    "--delay-between-chunks", $DelayBetweenChunks
)

if ($UseVertexAuthForGemini -and $TtsService -eq "gemini") {
    $PythonParams += "--use-vertex-auth-for-gemini"
    $GCloudProjectID = $env:GCLOUD_PROJECT_ID
    if (-not $GCloudProjectID) {
        Write-Error "GCLOUD_PROJECT_ID not found. Please add it to your .env file."
        exit 1
    }
    $PythonParams += "--gcloud-project", $GCloudProjectID
    Write-Host "--- AUTH MODE: Vertex AI (Production Quotas for Gemini) ---" -ForegroundColor Green
    Write-Host "Using Google Cloud Project: $GCloudProjectID (from .env)"
}


# Add service-specific parameters
if ($TtsService -eq "gemini") {
    $PythonParams += "--model-name", $GeminiModelName
    $PythonParams += "--tts-prompt-prefix", $GeminiTtsPromptPrefix
    
    # --- THIS IS THE FIX ---
    # Add the --voice-name parameter, then loop through the array to add each voice as a separate argument.
    $PythonParams += "--voice-name"
    foreach ($voice in $GeminiVoiceName) {
        $PythonParams += $voice
    }
    # --- END OF FIX ---

    Write-Host "Using GEMINI TTS service."
    Write-Host "Gemini Model: $GeminiModelName"
    $VoiceListString = $GeminiVoiceName -join ', '
    Write-Host "Gemini Voice(s): $VoiceListString"

} elseif ($TtsService -eq "vertex") {
    $PythonParams += "--voice-name", $VertexVoiceName
    $PythonParams += "--language-code", $VertexLanguageCode
    Write-Host "Using VERTEX AI TTS service (for older Chirp/WaveNet voices)."
    Write-Host "Vertex Voice: $VertexVoiceName"
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

# --- Run the Command ---
Write-Host "---"
Write-Host "Running: python $PythonScriptPath $($PythonParams -join ' ')"
python $PythonScriptPath $PythonParams

$ExitCode = $LASTEXITCODE
if ($ExitCode -eq 0) {
    Write-Host "Python script finished successfully."
    $BaseAudioDir = "E:/Bill/Documents/development/audiolingual/audio"
    $ExpectedOutputFile = "$BaseAudioDir/$($InputFileName -replace '\.txt$', ".$OutputAudioFormat")"
    Write-Host "Check for '$ExpectedOutputFile'"
} else {
    Write-Host "Python script exited with error code: $ExitCode"
}

Read-Host -Prompt "Press Enter to exit"