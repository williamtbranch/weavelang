# debug_spacy.py
import spacy
import sys

# --- SpaCy Model Loader ---
def load_spacy_model(model_name: str):
    """Loads a SpaCy model, providing clear download instructions on failure."""
    try:
        print(f"Loading SpaCy model '{model_name}'... This may take a moment.")
        return spacy.load(model_name)
    except IOError:
        print("\n---", file=sys.stderr)
        print(f"ERROR: SpaCy model '{model_name}' not found.", file=sys.stderr)
        print("Please run this command to download it:", file=sys.stderr)
        print(f"python -m spacy download {model_name}", file=sys.stderr)
        sys.exit(1)

# --- Main Analysis ---
if __name__ == "__main__":
    # Load the Spanish model
    nlp = load_spacy_model("es_core_news_lg")
    print("Model loaded successfully.")

    # --- THIS IS THE NEW SENTENCE FROM THE TEST ---
    text = 'Él le preguntó a su amigo: "¿Vienes a la fiesta?"'
    doc = nlp(text)

    print(f"\n--- Syntactic Analysis for: '{text}' ---")
    print(f"{'Text':<12} | {'Lemma':<12} | {'POS':<8} | {'Dep':<10} | {'Head Text':<12} | {'Head POS':<8} | {'Children'}")
    print("-" * 90)

    for token in doc:
        children = [child.text for child in token.children]
        print(
            f"{token.text:<12} | "
            f"{token.lemma_:<12} | "
            f"{token.pos_:<8} | "
            f"{token.dep_:<10} | "
            f"{token.head.text:<12} | "
            f"{token.head.pos_:<8} | "
            f"[{', '.join(children)}]"
        )

    print("\n--- Applying current segmentation logic from helper.py ---")
    split_points = set()
    for token in doc:
        if token.dep_ in ("cc", "mark"):
            split_points.add(token.i)
        if token.dep_ == "ccomp" and token.i > 0:
            split_points.add(token.i)
        if token.pos_ == "ADP" and token.head.pos_ in ["VERB", "NOUN", "PROPN"]:
            if token.i > 0:
                split_points.add(token.i)
    
    print(f"Calculated split points (token indices): {sorted(list(split_points))}")
    num_segments = len(sorted(list(split_points))) + 1 if split_points else 1
    print(f"This would result in {num_segments} segment(s).")
    if not split_points:
        print("Conclusion: No split points were found with the current rules.")
    else:
        print("Split point tokens are:")
        for i in sorted(list(split_points)):
            print(f" - Index {i}: '{doc[i].text}' (Dep: {doc[i].dep_}, POS: {doc[i].pos_})")