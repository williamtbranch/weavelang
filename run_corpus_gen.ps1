# run_corpus_gen.ps1

# Optional: Navigate to your project directory if the script isn't already there
# cd "E:\Bill\development\weavelang"

Write-Host "Starting WeaveLang Corpus Generation..."

# Your cargo command
cargo run --release -- generate `
    --sequence "sequence.txt" `
    --tts_output_dir "E:/Bill/Documents/development/audiolingual/generated_tts_input" `
    --profiles_dir "E:/Bill/Documents/development/audiolingual/generated_profiles" `
    --sentences_per_block 200 `
    --max_regen_attempts_per_block 20 `
    --target_ct_threshold 0.98 `
    --max_words_to_activate_per_regen 3
    # Add --start_profile "path/to/profile.json" when needed

Write-Host "Corpus generation command finished."
# Pause at the end to see output if running from explorer (optional)
# Read-Host -Prompt "Press Enter to exit"