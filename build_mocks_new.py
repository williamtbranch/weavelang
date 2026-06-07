import json
import re
import os

json_file = "E:/Bill/development/weavelang/test_case/Metamorphosis_12.json"
out_dir = "E:/Bill/development/weavelang/test_case/test_01/LLM_responses"

with open(json_file, encoding='utf-8') as f:
    d = json.load(f)

blocks = [b for b in d['content_blocks'] if b.get('block_type') == 'sentence']

translate_text = []
simplify_segments = []
translate_text_basic = []
simplify_to_basic_english = []
generate_phrase_map = []
generate_inverse_phrase_map = []

def get_words(text):
    words = []
    for m in re.finditer(r"[\w]+|[^\s\w]+", text):
        if re.match(r"\w", m.group(0)):
            words.append(m.group(0))
    return words

for block in blocks:
    sid = block['s_id']
    tiers = {t['tier_id']: t for t in block.get('tiers', [])}
    
    if 'advanced_target' in tiers:
        adv_text = "".join(seg['text'] for seg in tiers['advanced_target']['segments']).strip()
        translate_text.append(f"{sid}: {adv_text}")
    
    if 'moderate_target' in tiers:
        for i, seg in enumerate(tiers['moderate_target']['segments']):
            simplify_segments.append(f"{sid}_S{i+1}: {seg['text'].strip()}")
            
    if 'basic_target' in tiers:
        bas_text = "".join(seg['text'] for seg in tiers['basic_target']['segments']).strip()
        translate_text_basic.append(f"{sid}: {bas_text}")
        
    if 'basic_base' in tiers:
        bas_base_text = "".join(seg['text'] for seg in tiers['basic_base']['segments']).strip()
        simplify_to_basic_english.append(f"{sid}: {bas_base_text}")
        
        bb_words = get_words(bas_base_text)
        mapping = block.get('mappings', {}).get('basic_spanish_to_basic_english_diglot', {}).get(sid, [])
        pm_lines = [f"{sid}:", "MAPPINGS:"]
        for m in mapping:
            idx = m[0]
            target = m[2]
            if target:
                if idx < len(bb_words):
                    pm_lines.append(f"{bb_words[idx]} -> {target}")
                else:
                    pm_lines.append(f"WORD_{idx} -> {target}")
        pm_lines.append(f"VALIDATION: {bas_base_text}\n")
        generate_phrase_map.append("\n".join(pm_lines))
        
    if 'basic_target' in tiers:
        bas_tgt_text = "".join(seg['text'] for seg in tiers['basic_target']['segments']).strip()
        bt_words = get_words(bas_tgt_text)
        mapping = block.get('mappings', {}).get('basic_target_to_basic_base_inv_diglot', {}).get(sid, [])
        ipm_lines = [f"{sid}:", "MAPPINGS:"]
        for m in mapping:
            idx = m[0]
            target = m[2]
            if target:
                if idx < len(bt_words):
                    ipm_lines.append(f"{bt_words[idx]} -> {target}")
                else:
                    ipm_lines.append(f"WORD_{idx} -> {target}")
        ipm_lines.append(f"VALIDATION: {bas_tgt_text}\n")
        generate_inverse_phrase_map.append("\n".join(ipm_lines))

def write_file(name, lines):
    with open(f"{out_dir}/{name}", "w", encoding='utf-8', newline='\n') as f:
        f.write("\n".join(lines) + "\n")

write_file('advanced.txt', translate_text)
write_file('moderate.txt', simplify_segments)
write_file('basic_target.txt', translate_text_basic)
write_file('basic_base.txt', simplify_to_basic_english)
write_file('basic_diglot.txt', generate_phrase_map)
write_file('inverse_diglot.txt', generate_inverse_phrase_map)

print("Regenerated all LLM response files.")
