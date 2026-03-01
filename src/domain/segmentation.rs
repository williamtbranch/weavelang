use regex::Regex;
use std::collections::HashSet;

pub fn count_real_words(text: &str) -> usize {
    // PERF: Compiling regex every time is slow. Consider using lazy_static or once_cell.
    // But for now, direct port.
    let re = Regex::new(r"[a-zA-Z]+").unwrap();
    re.find_iter(text).count()
}

pub fn merge_short_segments(mut segments: Vec<String>, min_segment_words: usize) -> Vec<String> {
    if segments.is_empty() {
        return Vec::new();
    }

    let merge_forward_punct: HashSet<char> = ['.', '!', '?', ':', ';'].iter().cloned().collect();
    let opening_punct: HashSet<char> = ['“', '"', '‘', '(', '[', '{', '¡', '¿'].iter().cloned().collect();

    // Calculate initial word counts
    let mut word_counts: Vec<usize> = segments.iter().map(|s| count_real_words(s)).collect();

    loop {
            let mut min_idx = None;
            for (i, &count) in word_counts.iter().enumerate() {
                if count > 0 && count < min_segment_words {
                    min_idx = Some(i);
                    break;
                }
            }

            let min_idx = match min_idx {
                Some(idx) => idx,
                None => break,
            };

            let can_merge_backward = min_idx > 0;
            let can_merge_forward = min_idx < segments.len() - 1;
            let mut merge_backward = false; // logic sets this

            if !can_merge_backward && !can_merge_forward {
                break;
            } else if can_merge_backward && !can_merge_forward {
                merge_backward = true;
            } else if !can_merge_backward && can_merge_forward {
                merge_backward = false;
            } else {
                let left_neighbor = segments[min_idx - 1].trim();
                let right_neighbor_plus_one = segments[min_idx + 1].trim();
                
                let left_ends_with_merge_forward = left_neighbor.chars().last().map_or(false, |c| merge_forward_punct.contains(&c));
                let right_starts_with_opening = right_neighbor_plus_one.chars().next().map_or(false, |c| opening_punct.contains(&c));

                if left_ends_with_merge_forward {
                    merge_backward = false;
                } else if right_starts_with_opening {
                    merge_backward = true;
                } else {
                    merge_backward = word_counts[min_idx - 1] <= word_counts[min_idx + 1];
                }
            }

            if merge_backward {
                let current = segments.remove(min_idx);
                let current_count = word_counts.remove(min_idx);
                segments[min_idx - 1].push_str(&current);
                word_counts[min_idx - 1] += current_count;
            } else {
                let next = segments.remove(min_idx + 1);
                let next_count = word_counts.remove(min_idx + 1);
                segments[min_idx].push_str(&next);
                word_counts[min_idx] += next_count;
            }
        }

        segments.into_iter().filter(|s| !s.trim().is_empty()).collect()
    }
