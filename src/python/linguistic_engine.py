import sys
import json
import re
import unicodedata
import logging
import io

# Force standard I/O streams to UTF-8
sys.stdin = io.TextIOWrapper(sys.stdin.buffer, encoding='utf-8')
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')

logging.basicConfig(stream=sys.stderr, level=logging.INFO, format='[PyEngine] %(message)s')

# We keep this helper because SpaCy's Spanish lemmatizer output isn't always normalized 
# the way WeaveLang likes it (accents vs no accents).
def normalize_spanish_lemma(lemma_str: str) -> str:
    s = lemma_str.lower().strip().split(' ')[0]
    s = (s.replace('á', 'a').replace('é', 'e').replace('í', 'i').replace('ó', 'o').replace('ú', 'u').replace('ñ', 'n').replace('ü', 'u'))
    s = re.sub(r'^[^\w]+|[^\w]+$', '', s)
    if not s: return ""
    s = unicodedata.normalize('NFC', s)
    if re.search(r'[^a-z-]', s): return ""
    return s

def clean_and_flatten_text(text: str) -> str:
    """
    Cleans metadata (if present) and flattens text for robust Stanza segmentation.
    """
    # 1. Remove Gutenberg Headers/Footers if detected
    # (Simple heuristic based on standard markers)
    start_marker = "*** START OF THE PROJECT GUTENBERG EBOOK"
    end_marker = "*** END OF THE PROJECT GUTENBERG EBOOK"
    
    start_idx = text.find(start_marker)
    if start_idx != -1:
        # Find end of line of marker
        newline_pos = text.find('\n', start_idx)
        if newline_pos != -1:
            text = text[newline_pos+1:]
            
    end_idx = text.find(end_marker)
    if end_idx != -1:
        text = text[:end_idx]

    # 2. Remove illustration tags (using regex from gutenberg_cleaner.py)
    text = re.sub(r"\[(?:Illustration|Copyright|Etext|Project Gutenberg|PG|etext)[^\]\n]*?\]", " ", text, flags=re.IGNORECASE)

    # 3. Flatten: Replace newlines with spaces
    text = text.replace('\n', ' ').replace('\r', ' ')
    
    # 4. Collapse multiple spaces
    text = re.sub(r'\s+', ' ', text).strip()
    
    return text

# Known abbreviations that stanza may incorrectly treat as sentence-ending.
# Pattern: sentence ends with one of these followed by a period.
ABBREVIATION_PATTERN = re.compile(
    r'\b(?:Mrs|Mr|Ms|Dr|Prof|Rev|Gen|Sgt|Lt|Col|Capt|Cmdr|Maj|Cpl|Pvt'
    r'|Jr|Sr|St|vs|etc|approx|dept|est|govt|inc|ltd|vol|ch|fig|no'
    r'|Jan|Feb|Mar|Apr|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\.$',
    re.IGNORECASE
)

def merge_abbreviation_splits(sentences: list[str]) -> list[str]:
    """
    Post-process stanza output: merge consecutive sentences where the first
    ends with a known abbreviation (e.g. 'Mrs.', 'Mr.', 'Dr.') so that
    false sentence breaks at abbreviations are healed.
    """
    if len(sentences) <= 1:
        return sentences
    merged = []
    carry = ""
    for sent in sentences:
        if carry:
            # Merge: previous ended with abbreviation, join with this sentence
            merged.append(carry + " " + sent)
            carry = ""
        elif ABBREVIATION_PATTERN.search(sent):
            carry = sent
        else:
            merged.append(sent)
    if carry:
        merged.append(carry)  # trailing abbreviation sentence with no successor
    return merged

MODELS = {}

def get_model(lang_code):
    if lang_code in MODELS: return MODELS[lang_code]
    import spacy
    model_name = "es_core_news_lg" if lang_code == "es" else "en_core_web_lg"
    try:
        nlp = spacy.load(model_name, disable=["ner", "parser"])
        nlp.add_pipe("sentencizer") # Ensure sentence segmentation is available
        MODELS[lang_code] = nlp
        return nlp
    except OSError:
        raise ValueError(f"Model '{model_name}' not found.")

STANZA_PIPELINES = {}

def get_stanza_pipeline(lang_code):
    if lang_code in STANZA_PIPELINES: return STANZA_PIPELINES[lang_code]
    import stanza
    try:
        # Only load tokenization for segmentation to keep it fast/light
        nlp = stanza.Pipeline(lang_code, processors='tokenize', verbose=False, use_gpu=False)
        STANZA_PIPELINES[lang_code] = nlp
        return nlp
    except Exception as e:
        raise ValueError(f"Stanza pipeline for '{lang_code}' failed: {e}")

def main():
    logging.info("Linguistic Engine Started (Dumb Mode).")
    while True:
        try:
            line = sys.stdin.readline()
            if not line: break
            
            request = json.loads(line)
            action = request.get("action")
            response = {"status": "error", "data": None, "message": "Unknown Error"}

            if action == "tokenize":
                text = request.get("text", "")
                lang = request.get("lang", "en")
                try:
                    nlp = get_model(lang)
                    doc = nlp(text)
                    
                    # --- RAW SPACY DUMP ---
                    raw_tokens = []
                    for token in doc:
                        lemma = ""
                        if not token.is_punct and not token.is_space:
                            if lang == "es":
                                lemma = normalize_spanish_lemma(token.lemma_)
                            else:
                                lemma = token.lemma_.lower().strip()

                        raw_tokens.append({
                            "text": token.text,
                            "lemma": lemma,
                            "pos": token.pos_,
                            "is_punct": token.is_punct,
                            "is_space": token.is_space,
                            "whitespace": token.whitespace_
                        })
                    
                    response = {"status": "success", "tokens": raw_tokens}
                except Exception as e:
                    response = {"status": "error", "message": str(e)}

            elif action == "segment":
                text = request.get("text", "")
                lang = request.get("lang", "en")
                engine = request.get("engine", "stanza")
                
                try:
                    # Apply cleaning and flattening as requested
                    clean_text = clean_and_flatten_text(text)

                    sentences = []
                    if engine == "stanza":
                        nlp = get_stanza_pipeline(lang)
                        doc = nlp(clean_text)
                        sentences = [sentence.text for sentence in doc.sentences]
                    else: # fallback to spacy
                        nlp = get_model(lang)
                        doc = nlp(clean_text)
                        sentences = [sent.text for sent in doc.sents]
                    
                    # Post-process: heal abbreviation-based false splits
                    sentences = merge_abbreviation_splits(sentences)

                    response = {"status": "success", "sentences": sentences}
                except Exception as e:
                    response = {"status": "error", "message": str(e)}
            
            elif action == "ping":
                response = {"status": "success", "message": "pong"}
            else:
                response = {"status": "error", "message": f"Unknown action: {action}"}

            print(json.dumps(response))
            sys.stdout.flush()

        except json.JSONDecodeError:
            print(json.dumps({"status": "error", "message": "Invalid JSON"}))
            sys.stdout.flush()
        except KeyboardInterrupt: break
        except Exception as e:
            logging.error(f"Critical Loop Error: {e}")
            print(json.dumps({"status": "error", "message": f"Critical: {str(e)}"}))
            sys.stdout.flush()

if __name__ == "__main__":
    main()