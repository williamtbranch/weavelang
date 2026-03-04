import json
with open('E:/Bill/development/weavelang/test_case/Metamorphosis_12.json', encoding='utf-8') as f:
    d = json.load(f)

for block in d['content_blocks']:
    if block['block_type'] == 'sentence':
        print(f"ID: {block['s_id']}")
        tiers = block.get('tiers', {})
        print(f"  Tiers: {list(tiers.keys())}")
        for tier_name, tier_data in tiers.items():
            if 'segments' in tier_data:
                print(f"  {tier_name} segments: {len(tier_data['segments'])}")
                print(f"    first seg: {tier_data['segments'][0]['text']}")
        print(f"  Maps: {list(block.get('mappings', {}).keys())}")
        break  # just first sentence
