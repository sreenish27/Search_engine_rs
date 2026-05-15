use std::collections::{BTreeMap, HashMap, HashSet};
use crate::block_merge::TermEntry;

// Spell correction via trigram candidates + Jaccard filter + Levenshtein ranking.
//
// Ranking tiebreaker (the key insight):
//   When multiple candidates have the same edit distance, the more frequent
//   term wins. `instance` (doc_freq ~5000) beats `instace` (doc_freq 1) every
//   time, even though both are edit-distance 1 from `instnace`.
//
// Plus a minimum doc_freq threshold so rare typos in the corpus can never
// be suggested as corrections.

/// Minimum doc frequency for a term to be a valid spell-correction candidate.
/// Below this, the term is almost certainly itself a typo or rare junk token
/// and suggesting it would mislead the user.
const MIN_CANDIDATE_DOC_FREQ: u32 = 3;

/// Build the set of trigrams for a term (char-based, never panics on Unicode).
fn three_gram_set(term: &str) -> HashSet<String> {
    let padded: Vec<char> = std::iter::once('$')
        .chain(term.chars())
        .chain(std::iter::once('$'))
        .collect();
    let mut grams = HashSet::new();
    if padded.len() < 3 {
        return grams;
    }
    for window in padded.windows(3) {
        grams.insert(window.iter().collect::<String>());
    }
    grams
}

/// Spell-correct a term. Pipeline:
///   1. trigram candidate lookup (any term sharing >=1 trigram)
///   2. drop candidates below MIN_CANDIDATE_DOC_FREQ (rare = probably-typo)
///   3. Jaccard filter (rough trigram overlap)
///   4. Levenshtein edit distance filter
///   5. rank by (edit_distance ASC, doc_freq DESC)
///
/// Returns candidates sorted best-first.
pub fn spell_corrector(
    term: &str,
    tri_gram_index: &BTreeMap<String, Vec<String>>,
    term_index: &HashMap<String, TermEntry>,
) -> Vec<String> {
    // 1. trigram candidates
    let query_trigrams = three_gram_set(term);
    let mut all_terms: HashSet<String> = HashSet::new();
    for trigram in &query_trigrams {
        if let Some(terms) = tri_gram_index.get(trigram) {
            all_terms.extend(terms.iter().cloned());
        }
    }
    println!(
        "  Spell check: '{}' -> {} raw candidates from trigram index",
        term,
        all_terms.len()
    );

    // 2. drop candidates below minimum doc frequency
    let before_freq = all_terms.len();
    all_terms.retain(|t| {
        term_index
            .get(t)
            .map(|entry| entry.doc_freq >= MIN_CANDIDATE_DOC_FREQ)
            .unwrap_or(false)
    });
    let dropped_by_freq = before_freq - all_terms.len();
    if dropped_by_freq > 0 {
        println!(
            "  Spell check: '{}' -> dropped {} rare candidates (doc_freq < {})",
            term, dropped_by_freq, MIN_CANDIDATE_DOC_FREQ
        );
    }

    // 3. Jaccard similarity filter
    let mut jaccard_keep: Vec<String> = Vec::new();
    for candidate in &all_terms {
        if jaccard_distance(term, candidate) > 0.3 {
            jaccard_keep.push(candidate.clone());
        }
    }
    println!(
        "  Spell check: '{}' -> {} after Jaccard filter",
        term,
        jaccard_keep.len()
    );

    // 4. Levenshtein filter (edit distance < half the term length)
    let max_edit = term.chars().count() / 2;
    let mut scored: Vec<(String, usize, u32)> = Vec::new();
    for candidate in jaccard_keep.iter() {
        let dist = edit_distance(term, candidate);
        if dist < max_edit {
            let freq = term_index
                .get(candidate)
                .map(|e| e.doc_freq)
                .unwrap_or(0);
            scored.push((candidate.clone(), dist, freq));
        }
    }
    println!(
        "  Spell check: '{}' -> {} after edit distance filter",
        term,
        scored.len()
    );

    // 5. rank by (edit_distance ASC, doc_freq DESC)
    //    Lower edit distance wins. Ties broken by higher document frequency.
    scored.sort_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)));

    scored.into_iter().map(|(t, _, _)| t).collect()
}

/// Jaccard distance = |intersection| / |union| of two terms' trigram sets.
pub fn jaccard_distance(term1: &str, term2: &str) -> f64 {
    let g1 = three_gram_set(term1);
    let g2 = three_gram_set(term2);
    let inter = g1.intersection(&g2).count();
    let union = g1.union(&g2).count();
    if union == 0 {
        return 0.0;
    }
    inter as f64 / union as f64
}

/// Levenshtein edit distance (char-based). O(m*n) time, O(n) space.
pub fn edit_distance(term1: &str, term2: &str) -> usize {
    let a: Vec<char> = term1.chars().collect();
    let b: Vec<char> = term2.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut curr = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            if a[i - 1] == b[j - 1] {
                curr[j] = prev[j - 1];
            } else {
                curr[j] = 1 + prev[j].min(curr[j - 1]).min(prev[j - 1]);
            }
        }
        prev = curr;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_entry(freq: u32) -> TermEntry {
        TermEntry {
            tiers: [(0, 0); 3],
            doc_freq: freq,
        }
    }

    #[test]
    fn frequency_breaks_ties_in_edit_distance() {
        // Both "instance" and "instace" are 1 edit away from "instnace".
        // With frequency-weighted ranking, "instance" (freq=5000) must win
        // over "instace" (freq=1).
        let mut term_index = HashMap::new();
        term_index.insert("instance".to_string(), dummy_entry(5000));
        term_index.insert("instace".to_string(), dummy_entry(1));

        let mut gram_index: BTreeMap<String, Vec<String>> = BTreeMap::new();
        // Both candidates must be findable via shared trigrams with "instnace".
        // Easiest: register both terms in a trigram set that overlaps the query.
        for t in &["instance", "instace"] {
            for w in three_gram_set(t) {
                gram_index.entry(w).or_default().push(t.to_string());
            }
        }

        let suggestions = spell_corrector("instnace", &gram_index, &term_index);
        assert!(!suggestions.is_empty(), "expected at least one suggestion");
        assert_eq!(
            suggestions[0], "instance",
            "expected 'instance' to outrank 'instace' on frequency, got: {:?}",
            suggestions
        );
    }

    #[test]
    fn rare_candidates_below_threshold_are_dropped() {
        // A candidate with doc_freq < MIN_CANDIDATE_DOC_FREQ should not be suggested
        // even if it's the only one available.
        let mut term_index = HashMap::new();
        term_index.insert("instace".to_string(), dummy_entry(1)); // below threshold

        let mut gram_index: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for w in three_gram_set("instace") {
            gram_index.entry(w).or_default().push("instace".to_string());
        }

        let suggestions = spell_corrector("instnace", &gram_index, &term_index);
        assert!(
            suggestions.is_empty(),
            "expected no suggestions when only candidate is below freq threshold, got: {:?}",
            suggestions
        );
    }

    #[test]
    fn higher_edit_distance_loses_even_if_more_frequent() {
        // Edit distance is the primary sort. A high-freq but-far candidate
        // should NOT beat a low-freq close candidate.
        let mut term_index = HashMap::new();
        term_index.insert("instance".to_string(), dummy_entry(10)); // edit-dist 1 from instnace
        term_index.insert("invoice".to_string(), dummy_entry(5000)); // edit-dist 4 from instnace

        let mut gram_index: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for t in &["instance", "invoice"] {
            for w in three_gram_set(t) {
                gram_index.entry(w).or_default().push(t.to_string());
            }
        }

        let suggestions = spell_corrector("instnace", &gram_index, &term_index);
        assert_eq!(suggestions.first().map(String::as_str), Some("instance"));
    }

    #[test]
    fn trigram_set_handles_unicode_without_panic() {
        let grams = three_gram_set("numberofmessagesdeleted\u{00B9}");
        assert!(!grams.is_empty());
    }

    #[test]
    fn trigram_set_handles_empty() {
        assert_eq!(three_gram_set(""), HashSet::new());
    }

    #[test]
    fn jaccard_identical_is_one() {
        assert_eq!(jaccard_distance("cat", "cat"), 1.0);
    }

    #[test]
    fn jaccard_disjoint_is_zero() {
        assert_eq!(jaccard_distance("cat", "xyz"), 0.0);
    }

    #[test]
    fn edit_distance_identical_is_zero() {
        assert_eq!(edit_distance("kubernetes", "kubernetes"), 0);
    }

    #[test]
    fn edit_distance_one_substitution() {
        assert_eq!(edit_distance("kubernetes", "kubernetis"), 1);
    }
}