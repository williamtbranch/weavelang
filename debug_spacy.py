# In debug_spacy.py
import spacy
nlp = spacy.load("en_core_web_lg")
text = "He said,  “It’s great!”"
doc = nlp(text)

print("SpaCy Tokens:")
for token in doc:
    print(f"'{token.text}'")