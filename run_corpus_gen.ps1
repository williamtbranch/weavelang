# run_corpus_gen.ps1

# Optional: Navigate to your project directory if the script isn't already there
# cd "E:\Bill\development\weavelang"

Write-Host "Starting WeaveLang Corpus Generation..."

# Your cargo command
cargo run  -- generate `
    --sequence "sequence.txt" `
    --tts-output-dir "E:/Bill/Documents/development/audiolingual/generated_tts_input" `
    --profiles-dir "E:/Bill/Documents/development/audiolingual/generated_profiles" `
    --sentences-per-block 200 `
    --max-regen-attempts-per-block 20 `
    --target-ct-threshold 0.98 `
    --max-words-to-activate-per-regen 3
    # Add --start_profile "path/to/profile.json" when needed

Write-Host "Corpus generation command finished."
# Pause at the end to see output if running from explorer (optional)
# Read-Host -Prompt "Press Enter to exit"