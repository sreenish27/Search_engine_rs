use std::{char::MAX, cmp::Reverse, collections::{BinaryHeap, HashMap}, f32::MIN};
use std::time::Instant;
use crate::{block_merge::TermEntry, get_posting::read_postings};

// Toggle diagnostic prints on/off. Set to false to measure pure scoring time.
const VERBOSE: bool = true;

//a class of functions to calculate the tf_idf scores and rank the search results using it

//term frequency - function
pub fn tf_idf(t_f: f32, n: f32, df: f32) -> f32 {
    if t_f == 0.0 {
        return 0.0;
    }
    //apply tf_idf formula
    let tf = 1.0 + t_f.log10();
    let idf = (n/df).log10();

    tf*idf
}

//a function to assist omega function - ingests a list of lists (list of positional index for a particular doc_id across terms)
//initialize pointer - and find omega basically - but want to keep it clean and seperate
/// Compute the smallest window that contains at least one position from each term.
/// Caller must guarantee:
///   - k >= 2 (multi-term query only; no window boost for single-term queries).
///   - every position list is non-empty (doc has at least one occurrence per term).
pub fn window_calc(position_lists: &[&[u32]]) -> u32 {
    let k = position_lists.len();
    if k == 0 {
        return 0;
    }

    debug_assert!(k >= 2, "window_calc called with k < 2; should be skipped in rank_results");
    for slice in position_lists {
        debug_assert!(!slice.is_empty(), "window_calc called with empty position list");
    }

    let mut pointers = vec![0usize; k];
    let mut best = u32::MAX;

    loop {
        let mut min = u32::MAX;
        let mut max = 0u32;
        let mut min_idx = 0usize;

        for i in 0..k {
            let v = position_lists[i][pointers[i]];
            if v < min {
                min = v;
                min_idx = i;
            }
            if v > max {
                max = v;
            }
        }

        best = best.min(max - min + 1);

        pointers[min_idx] += 1;
        if pointers[min_idx] == position_lists[min_idx].len() {
            return best;
        }
    }
}
//a function to calculate ω (omega) - the smallest window in which all terms in user query exists and no of terms 
//- put in a function and give a value to be used to boost a doc - which is later used in ranking
//reason: we are replacing this strict doc selection - where all terms exists in exact order vs - finding relevant docs which
//have terms dispersed
pub fn omega_calc(terms: &Vec<String>, doc_id: u32, all_postings: &HashMap<String, HashMap<u32, Vec<u32>>>) -> u32 {
    let position_lists: Vec<&[u32]> = terms.iter()
        .filter_map(|term| all_postings.get(term)?.get(&doc_id).map(|v| v.as_slice()))
        .collect();
    
    window_calc(&position_lists)
}
//boost calc - a function which takes omega - and returns a boost value which should be multiplied with tf_idf index 
//- keeping it seperate because - it is subject to change in future
pub fn boost_calc(k: usize, omega: u32) -> f32 {
    1.0 + (k as f32 / omega as f32)
}
//a ranking function which uses tf - idf 
pub fn rank_results(results: Vec<u32>, term_index: &HashMap<String, TermEntry>, terms: &Vec<String>, total_docs:f32, doc_vec_len: &HashMap<u32, f32>, tier_idx: usize) -> Vec<(u32, f32)> {
    let no_of_terms = terms.len();
    let n_results = results.len();

    if VERBOSE {
        println!("--- RANKING (tier {}) ---", tier_idx);
        println!("  Candidate docs: {}", n_results);
        println!("  Query terms: {} ({:?})", no_of_terms, terms);
    }

    let t_postings = Instant::now();
    let mut all_postings: HashMap<String, HashMap<u32, Vec<u32>>> = HashMap::new();
    //get all required postings for each term and store - only once we retrieve and deserialize
    for term in terms {
        let posting = read_postings(&term, term_index, tier_idx).unwrap();
        all_postings.insert(term.clone(), posting);
    }
    if VERBOSE {
        println!("  Postings read for {} terms in {:?}", terms.len(), t_postings.elapsed());
    }

    //now do the rest
    //declare a variable to hold doc_id and final tf_idf score
    let t_score = Instant::now();
    let mut ranked_docs: Vec<(u32, f32)> = Vec::new();

    // running stats — accumulated, printed once at the end
    let mut sum_omega: u64 = 0;
    let mut min_omega: u32 = u32::MAX;
    let mut max_omega: u32 = 0;
    let mut sum_boost: f32 = 0.0;
    let mut docs_with_boost: usize = 0;

    for &doc_id in &results {
        let mut doc_score:f32 = 0.0;
        for term in terms {
            let posting = &all_postings[term];
            let t_f = posting[&doc_id].len() as f32;
            let d_f = term_index[term].doc_freq as f32;
            let score = tf_idf(t_f, total_docs, d_f);
            doc_score += score;
        }
        //before pushing - once I am done for a doc_id all terms - divide by the vector length for 
        //that doc_id which is already pre-computed and stored
        doc_score /= doc_vec_len[&doc_id];
        //omega - boost calc - normalization
        let b = if no_of_terms >= 2 {
            let omega = omega_calc(terms, doc_id , &all_postings);
            if VERBOSE {
                sum_omega += omega as u64;
                if omega < min_omega { min_omega = omega; }
                if omega > max_omega { max_omega = omega; }
                docs_with_boost += 1;
            }
            let boost = boost_calc(no_of_terms, omega);
            if VERBOSE {
                sum_boost += boost;
            }
            boost
        } else {
            1.0
        };

        //multiply with boost
        doc_score *= b;

        ranked_docs.push((doc_id, doc_score));
    }
    if VERBOSE {
        println!("  Scoring loop ({} docs) in {:?}", n_results, t_score.elapsed());

        if docs_with_boost > 0 {
            let avg_omega = sum_omega as f32 / docs_with_boost as f32;
            let avg_boost = sum_boost / docs_with_boost as f32;
            println!("  Omega:  min={}  max={}  avg={:.2}", min_omega, max_omega, avg_omega);
            println!("  Boost:  avg={:.3}  (1.0 = no effect, 2.0 = strict phrase match)", avg_boost);
        } else {
            println!("  Omega: skipped (single-term query, no proximity boost)");
        }
    }

    //sort them before giving the result
    let t_sort = Instant::now();
    ranked_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    if VERBOSE {
        println!("  Sort time: {:?}", t_sort.elapsed());

        println!("  Top 5 preview:");
        for (i, (doc_id, score)) in ranked_docs.iter().take(5).enumerate() {
            println!("    {}. doc_id={}  score={:.4}", i + 1, doc_id, score);
        }
        println!();
    }

    //final result
    ranked_docs
}