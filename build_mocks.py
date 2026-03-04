import json
import os

json_path = 'E:/Bill/development/weavelang/test_case/Metamorphosis_12.json'
out_dir = 'E:/Bill/development/weavelang/test_case/test_01/LLM_responses_golden'
os.makedirs(out_dir, exist_ok=True)

with open(json_path, encoding='utf-8') as f:
    d = json.load(f)

advanced_target_lines = []
moderate_target_lines = []
basic_target_lines = []
phrase_map_lines = []
inverse_phrase_map_lines = []

for block in d['content_blocks']:
    if block.get('block_type') == 'sentence':
        s_id = block['s_id']
        
        # 1. Tiers
        for tier in block.get('tiers', []):
            tier_id = tier['tier_id']
            if tier_id == 'advanced_target':
                # segments usually are list of dicts with 'text'
                full_text = "".join(seg['text'] for seg in tier['segments']).strip()
                advanced_target_lines.append(f"{s_id}: {full_text}")
            elif tier_id == 'moderate_target':
                for i, seg in enumerate(tier['segments']):
                    seg_text = seg['text'].strip()
                    moderate_target_lines.append(f"{s_id}_S{i+1}: {seg_text}")
            elif tier_id == 'basic_target':
                # basic_target is un-segmented or 1 segment usually? We just take entire text
                full_text = "".join(seg['text'] for seg in tier['segments']).strip()
                basic_target_lines.append(f"{s_id}: {full_text}")
                
        # 2. Mappings
        # GeneratePhraseMap expects format S1: \n MAPPINGS: \n <map> \n VALIDATION: <val> \n
        maps = block.get('mappings', {})
        if 'basic_english_to_basic_spanish_diglot' in maps:
            phrase_map_lines.append(f"{s_id}:\nMAPPINGS:")
            # In JSON, basic_english_to_basic_spanish_diglot has {s_id: [ [idx, lemmas, text, is_word, weight, _], ... ]}
            # Actually we just want `source -> target` format or whatever the prompt produces.
            # But the mock just reads the MAPPING text lines... Wait, how much of MAPPINGS does the Rust side actually parse? 
            # It parses "source -> target" to populate the Map.
            # Let's extract mappings from JSON. 
            pass

# We will fill the rest in after inspecting exactly what the JSON holds for maps
