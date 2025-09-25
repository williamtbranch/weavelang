# debug_tokenizer.py
import stanza
import pprint
from llm2books.helper import create_golden_token_stream

print("Initializing Stanza (this may take a moment)...")
try:
    # Ensure we use the exact same library the pipeline uses
    nlp = stanza.Pipeline('es', processors='tokenize', use_gpu=False, logging_level='WARN')
    print("Stanza pipeline loaded.")
except Exception as e:
    print(f"Could not load Stanza model. Make sure it's downloaded (`stanza.download('es')`). Error: {e}")
    exit()

# This is the exact text fragment that is causing the problem
# It represents the boundary between S15_S2 and S15_S3 from the log
problem_text = "lugar: pero la persona que manda dijo: 'Una cosa pequeña"

print(f"\n--- Testing problem text ---\n'{problem_text}'")

# Process with Stanza to get the raw components
doc = nlp(problem_text)
sentence = doc.sentences[0]

print("\n--- Raw Stanza Tokens/Words ---")
# Stanza has a nested structure of sentences -> tokens -> words
for token in sentence.tokens:
    print(f"Token: '{token.text}'")
    for word in token.words:
        print(f"  - Word: '{word.text}', Start: {word.start_char}, End: {word.end_char}")

print("\n--- Output of create_golden_token_stream ---")
# Call our function and see what it produces
golden_stream = create_golden_token_stream(sentence)

# Use pprint for a more readable output of the list of dicts
pprint.pprint(golden_stream)

# --- Verification ---
print("\n--- Verification ---")
word_tokens = [t['v'] for t in golden_stream if t['t'] == 'w']
print("Word tokens found:", word_tokens)

if "dijo'Una" in word_tokens:
    print("\n[!!!] BUG CONFIRMED: 'dijo' and 'Una' were incorrectly fused.")
else:
    print("\n[OK] 'dijo' and 'Una' were correctly separated.")