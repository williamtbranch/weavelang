import re
text = open('tests/integration_e2e.rs', 'r', encoding='utf-8').read()

new_block = '''    // 4. Verify Exported JSON against Golden JSON
    let weave_dir = test_dir.join("weave_output");
    let exported_json_path = weave_dir.join("exported.json");
    assert!(
        exported_json_path.exists(),
        "Expected exported JSON at {}",
        exported_json_path.display()
    );

    let golden_json_path = test_dir.parent().unwrap().join("Metamorphosis_12.json");
    let golden_str = std::fs::read_to_string(&golden_json_path).expect("Could not read golden JSON");
    let exported_str = std::fs::read_to_string(&exported_json_path).expect("Could not read exported JSON");

    let expected: JsonChapter = serde_json::from_str(&golden_str).expect("Failed to parse golden JSON");
    let actual: JsonChapter = serde_json::from_str(&exported_str).expect("Failed to parse exported JSON");

    let expected_sentences: Vec<_> = expected.content_blocks.iter().filter(|b| matches!(b, JsonContentBlock::Sentence(_))).collect();
    let actual_sentences: Vec<_> = actual.content_blocks.iter().filter(|b| matches!(b, JsonContentBlock::Sentence(_))).collect();

    assert_eq!(
        expected_sentences.len(),
        actual_sentences.len(),
        "Mismatch in number of sentence blocks!"
    );

    for (e_s, a_s) in expected_sentences.into_iter().zip(actual_sentences.into_iter()) {
        if let (JsonContentBlock::Sentence(e_sent), JsonContentBlock::Sentence(a_sent)) = (e_s, a_s) {
            assert_eq!(e_sent.s_id, a_sent.s_id, "Sentence ID mismatch");
            for e_tier in &e_sent.tiers {
                let a_tier = a_sent.tiers.iter().find(|t| t.tier_id == e_tier.tier_id)
                    .unwrap_or_else(|| panic!("Tier {} missing in sentence {}", e_tier.tier_id, e_sent.s_id));
                assert_eq!(
                    e_tier.full_text, 
                    a_tier.full_text,
                    "Text mismatch in tier {} of sentence {}", e_tier.tier_id, e_sent.s_id
                );
            }
        }
    }
    eprintln!("[e2e] JSON Chapter comparison: PASS");

    // Clean up generated test output'''

text = re.sub(r'    // 4\. Verify generated weave files exist.*// Clean up generated test output', new_block, text, flags=re.DOTALL)
open('tests/integration_e2e.rs', 'w', encoding='utf-8').write(text)
