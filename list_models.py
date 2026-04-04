import google.generativeai as genai
import os

try:
    import keyring
except ImportError:
    keyring = None

# Try OS keyring first (matches Rust app), then GOOGLE_API_KEY env var
api_key_to_use = None
if keyring:
    try:
        api_key_to_use = keyring.get_password("google_api_key.weavelang", "google_api_key")
    except Exception:
        pass
if not api_key_to_use:
    api_key_to_use = os.getenv("GOOGLE_API_KEY")

if not api_key_to_use:
    print("ERROR: GOOGLE_API_KEY not found. Set via the app (set key google ...) or GOOGLE_API_KEY env var.")
else:
    try:
        genai.configure(api_key=api_key_to_use)
        print("\nAvailable models that support 'generateContent':")
        for m in genai.list_models():
            if 'generateContent' in m.supported_generation_methods:
                print(f"- {m.name}")
        print("\nFull list of all models (some may not support generateContent):")
        for m in genai.list_models():
            print(f"- Name: {m.name}, Display Name: {m.display_name}, Supported Methods: {m.supported_generation_methods}")

    except Exception as e:
        print(f"An error occurred: {e}")