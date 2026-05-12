// In src/simulation/frequency_manager.rs

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::domain::stemmer::{self, Stemmer};
use crate::domain::wlemma::BucketRanks;

struct FrequencyData {
    lemma_to_rank: HashMap<String, u32>,
    rank_to_lemma: Vec<String>,
    /// Wlemma bucket → minimum rank among all lemmas mapping to it.
    /// See `documentation/Wlemma_Migration_Plan.md`.
    bucket_rank: HashMap<String, u32>,
    /// Stemmer used to build `bucket_rank`; retained so ad-hoc lookups
    /// (`rank_of_lemma_string`) can stem at query time.
    stemmer: Box<dyn Stemmer>,
}

static FREQUENCY_DATA: Lazy<Mutex<Option<FrequencyData>>> = Lazy::new(|| Mutex::new(None));
static LOADED_PATH: Lazy<Mutex<Option<PathBuf>>> = Lazy::new(|| Mutex::new(None));

/// Build the wlemma → min-rank map by stemming every lemma in the frequency
/// list and aggregating with min(). Lemmas the stemmer cannot reduce (no
/// stemmer registered for the language) are passed through identity-style.
fn build_bucket_rank(
    lemma_to_rank: &HashMap<String, u32>,
    stemmer: &dyn Stemmer,
) -> HashMap<String, u32> {
    let mut bucket: HashMap<String, u32> = HashMap::with_capacity(lemma_to_rank.len() / 2);
    for (lemma, &rank) in lemma_to_rank {
        let key = stemmer.stem(&lemma.trim().to_lowercase());
        bucket
            .entry(key)
            .and_modify(|r| {
                if rank < *r {
                    *r = rank;
                }
            })
            .or_insert(rank);
    }
    bucket
}

/// Identity stemmer used as a no-op fallback when no language stemmer is
/// registered. Keeps the bucket map well-defined for unsupported languages.
struct IdentityStemmer;
impl Stemmer for IdentityStemmer {
    fn stem(&self, word: &str) -> String {
        word.to_string()
    }
}

/// Diagnostic: log the top-N buckets by rank-spread. A large spread means
/// a common form shares a bucket with rare ones — exactly the lookups the
/// wlemma fix is rescuing.
fn log_top_spread_buckets(
    lemma_to_rank: &HashMap<String, u32>,
    bucket_rank: &HashMap<String, u32>,
    stemmer: &dyn Stemmer,
    top_n: usize,
) {
    let mut bucket_max: HashMap<&str, u32> = HashMap::with_capacity(bucket_rank.len());
    for (lemma, &rank) in lemma_to_rank {
        let key = stemmer.stem(&lemma.trim().to_lowercase());
        bucket_rank.get(&key).map(|_| {
            let entry = bucket_max
                .entry(bucket_rank.get_key_value(&key).unwrap().0.as_str())
                .or_insert(0);
            if rank > *entry {
                *entry = rank;
            }
        });
    }
    let mut spreads: Vec<(&str, u32, u32)> = bucket_max
        .iter()
        .map(|(k, &maxr)| {
            let minr = *bucket_rank.get(*k).unwrap();
            (*k, minr, maxr - minr)
        })
        .collect();
    spreads.sort_by(|a, b| b.2.cmp(&a.2));
    println!(
        "[INFO] Top {} wlemma buckets by rank spread (min_rank → spread):",
        top_n.min(spreads.len())
    );
    for (key, minr, spread) in spreads.iter().take(top_n) {
        println!("[INFO]   {:>10}  min={:>6}  spread={}", key, minr, spread);
    }
}

pub fn load_master_frequency_list(asset_path: &Path) -> Result<(), String> {
    let mut guard = FREQUENCY_DATA.lock().unwrap();
    let mut path_guard = LOADED_PATH.lock().unwrap();

    if let Some(loaded_path) = path_guard.as_ref() {
        if loaded_path == asset_path && guard.is_some() {
            return Ok(());
        }
    }

    println!(
        "[INFO] Loading master frequency list from: {}",
        asset_path.display()
    );
    let file = File::open(asset_path).map_err(|e| {
        format!(
            "Failed to open frequency list at '{}': {}",
            asset_path.display(),
            e
        )
    })?;
    let reader = BufReader::new(file);

    let mut temp_data: Vec<(String, u32)> = Vec::new();
    let mut lines_read = 0;
    let mut valid_lines_parsed = 0;

    for (i, line_result) in reader.lines().skip(1).enumerate() {
        lines_read += 1;
        let line = match line_result {
            Ok(l) => l,
            Err(_) => {
                eprintln!("[WARN] Failed to read line {} of frequency list.", i + 2);
                continue;
            }
        };

        let parts: Vec<&str> = line.split('\t').collect();

        if parts.len() >= 2 {
            let lemma = parts[0].trim().to_string();
            if let Ok(rank) = parts[1].parse::<u32>() {
                if !lemma.is_empty() {
                    temp_data.push((lemma, rank));
                    valid_lines_parsed += 1;
                }
            }
        }
    }

    println!(
        "[DEBUG] Frequency List Parser: Read {lines_read} data lines, successfully parsed {valid_lines_parsed} valid entries."
    );

    if temp_data.is_empty() {
        return Err("Frequency list is empty or could not be parsed.".to_string());
    }

    let mut lemma_to_rank = HashMap::new();
    let mut rank_to_lemma_temp: Vec<(u32, String)> = Vec::new();

    for (lemma, rank) in temp_data {
        lemma_to_rank.insert(lemma.clone(), rank);
        rank_to_lemma_temp.push((rank, lemma));
    }

    rank_to_lemma_temp.sort_by_key(|k| k.0);
    let rank_to_lemma: Vec<String> = rank_to_lemma_temp.into_iter().map(|(_, s)| s).collect();

    println!(
        "[INFO] Loaded {} unique lemmas into frequency manager.",
        lemma_to_rank.len()
    );

    // Build the wlemma bucket map. Hardcoded to Spanish today; once
    // multi-language support lands this should be driven by the active
    // project language. See Wlemma_Migration_Plan.md (Language Neutrality).
    let active_lang = "es";
    let stemmer_box = stemmer::for_language(active_lang)
        .unwrap_or_else(|| Box::new(IdentityStemmer));
    let bucket_rank = build_bucket_rank(&lemma_to_rank, stemmer_box.as_ref());

    let total_lemmas = lemma_to_rank.len() as f64;
    let bucket_count = bucket_rank.len() as f64;
    let avg_lemmas_per_bucket = if bucket_count > 0.0 {
        total_lemmas / bucket_count
    } else {
        0.0
    };
    println!(
        "[INFO] Built wlemma bucket map: {} buckets ({} lemmas, avg {:.2} lemmas/bucket).",
        bucket_rank.len(),
        lemma_to_rank.len(),
        avg_lemmas_per_bucket
    );

    // T2.4: log the top-20 buckets by rank-spread (max_rank - min_rank).
    // These are the buckets where the wlemma fix has the biggest effect:
    // a common form sharing a bucket with very rare ones.
    log_top_spread_buckets(&lemma_to_rank, &bucket_rank, stemmer_box.as_ref(), 20);

    *guard = Some(FrequencyData {
        lemma_to_rank,
        rank_to_lemma,
        bucket_rank,
        stemmer: stemmer_box,
    });
    *path_guard = Some(asset_path.to_path_buf());

    Ok(())
}

pub fn get_ordered_lemmas() -> Vec<String> {
    let guard = FREQUENCY_DATA.lock().unwrap();
    guard
        .as_ref()
        .expect("Master frequency list has not been loaded.")
        .rank_to_lemma
        .clone()
}

#[deprecated(
    note = "Use rank_of_wlemma instead. Lemma-keyed lookup is brittle when \
            the upstream lemmatizer hallucinates surface forms — see \
            documentation/Wlemma_Migration_Plan.md."
)]
pub fn get_rank_for_lemma(lemma: &str) -> Option<u32> {
    let guard = FREQUENCY_DATA.lock().unwrap();
    guard
        .as_ref()
        .expect("Master frequency list has not been loaded.")
        .lemma_to_rank
        .get(lemma.trim())
        .copied()
}

/// Look up the rank of a wlemma (stemmed bucket key). This is the lookup
/// AVD scoring should use; `get_rank_for_lemma` is retained for legacy
/// call sites and will be deprecated as those migrate.
pub fn rank_of_wlemma(wlemma: &str) -> Option<u32> {
    let guard = FREQUENCY_DATA.lock().unwrap();
    guard
        .as_ref()
        .expect("Master frequency list has not been loaded.")
        .bucket_rank
        .get(wlemma)
        .copied()
}

/// Stem-and-bucket lookup for callers that have only a lemma string in
/// hand (no surface form). Equivalent to `rank_of_wlemma(stem(lemma))`.
/// This is the migration target for every legacy `get_rank_for_lemma`
/// site that doesn't already have access to a populated `wlemmas` vec.
pub fn rank_of_lemma_string(lemma: &str) -> Option<u32> {
    let guard = FREQUENCY_DATA.lock().unwrap();
    let data = guard
        .as_ref()
        .expect("Master frequency list has not been loaded.");
    let key = data.stemmer.stem(&lemma.trim().to_lowercase());
    data.bucket_rank.get(&key).copied()
}

/// Adapter exposing the loaded bucket map via the `BucketRanks` trait so
/// `compute_wlemma` can be called against the live frequency manager
/// without coupling its module to this one.
pub struct GlobalBucketRanks;

impl BucketRanks for GlobalBucketRanks {
    fn rank_of(&self, wlemma: &str) -> Option<u32> {
        rank_of_wlemma(wlemma)
    }
}

/// Inspect a lemma's wlemma bucket: returns the bucket key (stem), its
/// rank, and every other lemma in the loaded frequency list that maps
/// to the same bucket. Members are returned sorted by ascending rank
/// (most common first).
///
/// Used by the authoring/debug surfaces (terminal `wlemma` command,
/// GUI hover tooltips) so content creators can see exactly which
/// inflections share a bucket. Returns `None` if the bucket does not
/// exist in the loaded list.
pub fn inspect_bucket(lemma: &str) -> Option<BucketInspection> {
    let guard = FREQUENCY_DATA.lock().unwrap();
    let data = guard.as_ref()?;
    inspect_bucket_in(lemma, &data.lemma_to_rank, &data.bucket_rank, data.stemmer.as_ref())
}

/// Pure implementation of `inspect_bucket` taking explicit maps + stemmer.
/// Exposed for unit tests; production callers should use `inspect_bucket`.
pub fn inspect_bucket_in(
    lemma: &str,
    lemma_to_rank: &HashMap<String, u32>,
    bucket_rank: &HashMap<String, u32>,
    stemmer: &dyn Stemmer,
) -> Option<BucketInspection> {
    let key = stemmer.stem(&lemma.trim().to_lowercase());
    let rank = bucket_rank.get(&key).copied()?;
    let mut members: Vec<(String, u32)> = lemma_to_rank
        .iter()
        .filter(|(l, _)| stemmer.stem(&l.trim().to_lowercase()) == key)
        .map(|(l, r)| (l.clone(), *r))
        .collect();
    members.sort_by_key(|(_, r)| *r);
    Some(BucketInspection { wlemma: key, rank, members })
}

#[derive(Debug, Clone)]
pub struct BucketInspection {
    pub wlemma: String,
    pub rank: u32,
    /// (lemma, rank) pairs, sorted by rank ascending.
    pub members: Vec<(String, u32)>,
}

// --- NEW FUNCTION ---
/// Returns the highest rank in the loaded frequency list.
pub fn get_max_rank() -> u32 {
    let guard = FREQUENCY_DATA.lock().unwrap();
    // Since ranks are 1-based, the total number of lemmas is the max rank.
    guard
        .as_ref()
        .expect("Master frequency list has not been loaded.")
        .rank_to_lemma
        .len() as u32
}

#[cfg(test)]
mod tests {
    //! TT3: bucket-rank construction is direct min-aggregation. We test the
    //! pure builder against a fake `Stemmer` to avoid touching the global
    //! mutex or filesystem.
    use super::*;

    /// Test stemmer that drops a trailing 's' to merge plurals into singulars.
    struct DropTrailingS;
    impl Stemmer for DropTrailingS {
        fn stem(&self, word: &str) -> String {
            word.strip_suffix('s').unwrap_or(word).to_string()
        }
    }

    fn pairs(items: &[(&str, u32)]) -> HashMap<String, u32> {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect()
    }

    #[test]
    fn bucket_uses_min_rank_per_stem() {
        // "niño" rank 154, "niños" rank 52370 → bucket "niño" should be 154.
        let lemmas = pairs(&[("niño", 154), ("niños", 52_370)]);
        let bucket = build_bucket_rank(&lemmas, &DropTrailingS);
        assert_eq!(bucket.get("niño"), Some(&154));
        assert_eq!(bucket.len(), 1);
    }

    #[test]
    fn bucket_collapses_distinct_lemmas_to_min() {
        // Use lemma pairs whose plural form actually has a trailing 's'
        // that our toy DropTrailingS will collapse: niño/niños, gato/gatos.
        let lemmas = pairs(&[
            ("niño", 154),
            ("niños", 52_370),
            ("gato", 800),
            ("gatos", 9_999),
        ]);
        let bucket = build_bucket_rank(&lemmas, &DropTrailingS);
        assert_eq!(bucket.get("niño"), Some(&154));
        assert_eq!(bucket.get("gato"), Some(&800));
        assert_eq!(bucket.len(), 2);
    }

    #[test]
    fn unknown_wlemma_returns_none_via_trait() {
        // Exercise the BucketRanks-trait shape without touching globals.
        struct Local(HashMap<String, u32>);
        impl BucketRanks for Local {
            fn rank_of(&self, w: &str) -> Option<u32> {
                self.0.get(w).copied()
            }
        }
        let lemmas = pairs(&[("niño", 154)]);
        let bucket = build_bucket_rank(&lemmas, &DropTrailingS);
        let local = Local(bucket);
        assert_eq!(local.rank_of("niño"), Some(154));
        assert_eq!(local.rank_of("zzznotaword"), None);
    }

    #[test]
    fn identity_stemmer_keeps_lemmas_distinct() {
        // Without a real stemmer, every lemma is its own bucket; ranks pass
        // through unchanged.
        let lemmas = pairs(&[("niño", 154), ("niños", 52_370)]);
        let bucket = build_bucket_rank(&lemmas, &IdentityStemmer);
        assert_eq!(bucket.get("niño"), Some(&154));
        assert_eq!(bucket.get("niños"), Some(&52_370));
        assert_eq!(bucket.len(), 2);
    }

    #[test]
    fn inspect_bucket_in_returns_members_sorted_by_rank() {
        // T7.1: inspect_bucket exposes a bucket's stem, rank, and member list
        // for the authoring/debug surfaces.
        let lemmas = pairs(&[
            ("niño", 154),
            ("niños", 52_370),
            ("gato", 800),
        ]);
        let bucket = build_bucket_rank(&lemmas, &DropTrailingS);
        let insp = inspect_bucket_in("niños", &lemmas, &bucket, &DropTrailingS)
            .expect("bucket exists");
        assert_eq!(insp.wlemma, "niño");
        assert_eq!(insp.rank, 154);
        assert_eq!(insp.members, vec![
            ("niño".to_string(), 154),
            ("niños".to_string(), 52_370),
        ]);

        // Unknown lemma → None.
        assert!(inspect_bucket_in("zzznotaword", &lemmas, &bucket, &DropTrailingS).is_none());
    }
}
