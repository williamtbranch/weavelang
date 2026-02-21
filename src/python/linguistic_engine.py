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

MODELS = {}

def get_model(lang_code):
    if lang_code in MODELS: return MODELS[lang_code]
    import spacy
    model_name = "es_core_news_lg" if lang_code == "es" else "en_core_web_lg"
    try:
        nlp = spacy.load(model_name, disable=["ner", "parser"])
        MODELS[lang_code] = nlp
        return nlp
    except OSError:
        raise ValueError(f"Model '{model_name}' not found.")

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