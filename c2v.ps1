# c2v.ps1 - Create to Video

Write-Host "Starting video generation process..." -ForegroundColor Yellow

# --- Configuration ---
$PythonExecutable = ".\.venv\Scripts\python.exe"
$PythonScriptPath = ".\create_video.py"

# --- REQUIRED: Set the name of the book to process ---
# This MUST match the directory name inside `audiolingual/video/`
$BookToProcess = "Metamorphosis" 

# --- Sanity Check ---
if (-not $BookToProcess) {
    Write-Error "The `$BookToProcess` variable cannot be empty. Please specify the book name."
    exit 1
}

# --- Construct and Run Command ---
$CommandArgs = @($BookToProcess)

Write-Host "Running command:"
Write-Host "$PythonExecutable $PythonScriptPath $($CommandArgs -join ' ')"
Write-Host "---"

& $PythonExecutable $PythonScriptPath $CommandArgs

$ExitCode = $LASTEXITCODE

Write-Host "---"
Write-Host "Python script finished with exit code: $ExitCode"

if ($ExitCode -ne 0) {
    Write-Warning "The video generation script exited with an error. Check the output above for FFmpeg errors."
} else {
    Write-Host "Video generation complete." -ForegroundColor Green
}

Read-Host -Prompt "Press Enter to exit"