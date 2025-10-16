# b2a.ps1 (Updated to load Project ID from .env file)

# --- START OF NEW HELPER FUNCTION ---
# Helper function to parse a .env file and load variables into the environment.
function Import-EnvFile {
    param (
        [string]$Path = ".env"
    )
    if (Test-Path $Path) {
        Get-Content $Path | ForEach-Object {
            $line = $_.Trim()
            # Skip comments and empty lines
            if ($line -and $line -notmatch "^\s*#") {
                $parts = $line -split '=', 2
                if ($parts.Length -eq 2) {
                    $name = $parts[0].Trim()
                    $value = $parts[1].Trim()
                    # Remove optional quotes from the value
                    if (($value.StartsWith('"') -and $value.EndsWith('"')) -or ($value.StartsWith("'") -and $value.EndsWith("'"))) {
                        $value = $value.Substring(1, $value.Length - 2)
                    }
                    # Set the variable in the current script's environment
                    [System.Environment]::SetEnvironmentVariable($name, $value, "Process")
                    #Write-Host "Loaded from .env: $name" # Uncomment for debugging
                }
            }
        }
    } else {
        #Write-Warning ".env file not found at '$Path'" # Uncomment for debugging
    }
}
# --- END OF NEW HELPER FUNCTION ---


Write-Host "Starting Text-to-Speech conversion..."

# --- Load Environment Variables ---
# The function will load variables from .env into the current process.
Import-EnvFile -Path (Join-Path $PSScriptRoot ".env")


# --- Configuration ---
$ToolRootPath = "E:/Bill/development/weavelang" # Your actual path
$PythonScriptPath = "$ToolRootPath/book_to_audio.py"
$InputFileName = "Metamorphosis_UL14.txt" # Your test file

# --- TTS Service Selection ---
$TtsService = "gemini" 
[bool]$UseVertexAuthForGemini = $true 

# --- Gemini TTS Configuration ---
$GeminiModelName = "models/gemini-2.5-pro-preview-tts"
$GeminiVoiceName = "Charon"
$GeminiTtsPromptPrefix = "You are a professional voice actor with a Mexican Spanish accent."

# --- Vertex AI TTS Configuration ---
$VertexVoiceName = "es-US-Chirp3-HD-Achernar"
$VertexLanguageCode = "es-US"

# --- Run Mode ---
[bool]$RepairMode = $true

# --- Common Processing Parameters ---
$OutputAudioFormat = "wav"
$LogLevel = "INFO"
$ConcurrentRequests = 1
$ChunkMaxChars = 3000
$MaxApiRetries = 5
$RetryDelay = 20
$DelayBetweenChunks = 0 # Set back to 0 if your quota increase was approved

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

# --- MODIFIED: Logic to use Project ID from .env file ---
if ($UseVertexAuthForGemini -and $TtsService -eq "gemini") {
    $PythonParams += "--use-vertex-auth-for-gemini"
    
    # Read the GCLOUD_PROJECT_ID from the environment (loaded from .env)
    $GCloudProjectID = $env:GCLOUD_PROJECT_ID
    
    if (-not $GCloudProjectID) {
        Write-Error "GCLOUD_PROJECT_ID not found. Please add it to your .env file."
        exit 1
    }
    
    $PythonParams += "--gcloud-project", $GCloudProjectID
    Write-Host "--- AUTH MODE: Vertex AI (Production Quotas for Gemini) ---" -ForegroundColor Green
    Write-Host "Using Google Cloud Project: $GCloudProjectID (from .env)"
}
# --- END OF MODIFICATION ---


# Add service-specific parameters
if ($TtsService -eq "gemini") {
    $PythonParams += "--model-name", $GeminiModelName
    $PythonParams += "--voice-name", $GeminiVoiceName
    $PythonParams += "--tts-prompt-prefix", $GeminiTtsPromptPrefix
    Write-Host "Using GEMINI TTS service."
    Write-Host "Gemini Model: $GeminiModelName"
    Write-Host "Gemini Voice: $GeminiVoiceName"
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