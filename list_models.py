import google.generativeai as genai
import os
from dotenv import load_dotenv

# Load .env if you use it
dotenv_path = '.env'
if os.path.exists(dotenv_path):
    load_dotenv(dotenv_path=dotenv_path)
    print(f"INFO: Attempted to load API key from '{os.path.abspath(dotenv_path)}'.")

api_key_to_use = os.getenv("GOOGLE_API_KEY")

if not api_key_to_use:
    print("ERROR: GOOGLE_API_KEY not found in environment or .env file.")
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