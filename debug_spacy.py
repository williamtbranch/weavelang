import spacy

print("Loading SpaCy model...")
nlp = spacy.load("es_core_news_lg")
print("Model loaded.")

# Test the exact string from the JSON
text = "¡Explíquese!" 
doc = nlp(text)

print(f"\n--- Analyzing tokens for: '{text}' ---")
print(f"{'Text':<15} | {'Lemma':<15} | {'POS':<8} | {'is_punct?':<10} | {'is_space?':<10}")
print("-" * 70)

for token in doc:
    print(
        f"{token.text:<15} | "
        f"{token.lemma_:<15} | "
        f"{token.pos_:<8} | "
        f"{str(token.is_punct):<10} | "
        f"{str(token.is_space):<10}"
    )

print("\n--- Applying your filter ---")
lemmas = [
    token.lemma_ for token in doc 
    if not token.is_punct and not token.is_space and token.pos_ != "PROPN"
]
print(f"Resulting lemma list from filter: {lemmas}")