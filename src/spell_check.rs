use std::collections::{BTreeMap, HashSet};

//a class of functions - to do spell check using jaccard distance, three gram index and edit distance

//implementing my spell correction function - leverages trigram index to get possible words
//pipeline: trigram candidate lookup → Jaccard similarity filtering → Levenshtein edit distance ranking
pub fn spell_corrector(term: &str, tri_gram_index: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    //break the term into trigrams and put it in a list
    let mut trigram_list: Vec<String> = Vec::new();
    let mut i: usize = 0;
    let padded: String = format!("${}$", term);
    while i < padded.len() - 2 {
        trigram_list.push(padded[i..i + 3].to_string());
        i += 1;
    }
    //now get the trigrams list of terms from trigram_index and store it in a set
    let mut all_terms: HashSet<String> = HashSet::new();
    for trigram in &trigram_list {
        if let Some(terms) = tri_gram_index.get(trigram) {
            all_terms.extend(terms.iter().cloned());
        }
    }
    println!("  Spell check: '{}' → {} raw candidates from trigram index", term, all_terms.len());
    //now get the final candidates based on jaccard distance
    let mut candidates: Vec<(String, f64)> = Vec::new();
    for candidate in &all_terms {
        let score: f64 = jaccard_distance(term, candidate);
        if score > 0.3 {
            candidates.push((candidate.clone(), score));
        }
    }
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    println!("  Spell check: '{}' → {} after Jaccard filter", term, candidates.len());
    //now check for edit distance for these terms and then finally get the possible terms
    let mut possible_list: Vec<(String, usize)> = Vec::new();
    for (candidate, _score) in candidates.iter() {
        let e_distance: usize = edit_distance(term, candidate);
        if e_distance < term.len() / 2 {
            possible_list.push((candidate.clone(), e_distance));
        }
    }
    possible_list.sort_by_key(|a| a.1);
    println!("  Spell check: '{}' → {} after edit distance filter", term, possible_list.len());
    let final_list: Vec<String> = possible_list.iter().map(|(term, _)| term.clone()).collect();
    final_list
}

//jaccard distance = intersection / union of trigram sets between two terms
pub fn jaccard_distance(term1: &str, term2: &str) -> f64 {
    let grams1: HashSet<String> = three_gram_set(term1);
    let grams2: HashSet<String> = three_gram_set(term2);
    let intersection = grams1.intersection(&grams2).count();
    let union = grams1.union(&grams2).count();
    intersection as f64 / union as f64
}

//helper to get the set of trigrams for a term
pub fn three_gram_set(term: &str) -> HashSet<String> {
    let mut grams: HashSet<String> = HashSet::new();
    let padded = format!("${}$", term);
    let mut i: usize = 0;
    while i < padded.len() - 2 {
        grams.insert(padded[i..i + 3].to_string());
        i += 1;
    }
    grams
}

//edit distance (Levenshtein) - minimum insertions, deletions, substitutions to transform one string into another
//uses dynamic programming - O(m * n) where m and n are string lengths
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