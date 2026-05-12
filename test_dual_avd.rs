// Quick test to verify dual AVD scoring logic
// Compile and run: rustc test_dual_avd.rs && ./test_dual_avd

fn main() {
    // Sample lemma instances (rank,count) pairs
    // V1: with 0.2% cap (GREGOR EFFECT)
    // V2: without cap (natural distribution)
    
    let test_cases = vec![
        (vec![1,1,1, 2,2, 3, 5,5,5,5, 100, 500], "Basic Mix"),
        (vec![1,1,1, 2,2, 3,3, 10,10,10,10,10,10, 200,200], "Complex Mix"),
    ];
    
    for (lemmas, description) in test_cases {
        let total = lemmas.len() as f64;
        
        // Calculate tallies
        let mut rank_counts: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        for rank in &lemmas {
            *rank_counts.entry(*rank).or_insert(0) += 1;
        }
        
        // Create sorted rank,count pairs
        let mut tallies: Vec<(u32, u32)> = rank_counts.into_iter().collect();
        tallies.sort();
        
        // Calculate percentiles
        let mut cumulative = 0.0;
        let mut p85_rank = 0;
        let mut p95_rank = 0;
        
        for (rank, count) in &tallies {
            cumulative += *count as f64;
            let pct = (cumulative / total) * 100.0;
            if pct >= 85.0 && p85_rank == 0 {
                p85_rank = *rank;
            }
            if pct >= 95.0 && p95_rank == 0 {
                p95_rank = *rank;
            }
        }
        
        let avd_score = (p85_rank as f64 + 2.0 * p95_rank as f64) / 3.0;
        
        println!("{}: P85={}, P95={}, AVD={(avd_score:.2)}", description, p85_rank, p95_rank);
    }
}
