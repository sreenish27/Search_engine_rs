use std::collections::HashMap;
use crate::{block_merge::TermEntry, get_posting::read_postings};

//a couple of functions which captures checking if a sequence of words occur in order across documents
//checks if a phrase exists in a document by verifying consecutive positions
//term[0] at pos p, term[1] at p+1, term[2] at p+2, etc.
pub fn has_phrase(doc_id: u32, query_terms: &Vec<String>, all_postings: &Vec<HashMap<u32, Vec<u32>>>) -> bool {
    //get position list for the first term
    let first_positions = all_postings[0].get(&doc_id).unwrap();
    //for each starting position of the first term
    for &p in first_positions {
        let mut found = true;
        //check if every subsequent term exists at p+1, p+2, p+3...
        for (offset, _) in query_terms.iter().enumerate().skip(1) {
            let term_positions = all_postings[offset].get(&doc_id).unwrap();
            if term_positions.binary_search(&(p + offset as u32)).is_err() {
                found = false;
                break;
            }
        }
        if found {
            return true;
        }
    }
    false
}

//filters the doc list to only docs containing the exact phrase
//reads postings from disk once for all terms, then checks positional adjacency
pub fn phrase_filter(final_list: Vec<u32>, query_terms: &Vec<String>, term_index: &HashMap<String, TermEntry>, tier_idx: usize) -> Vec<u32> {
    //read all postings once from disk - don't read per document
    let mut all_postings: Vec<HashMap<u32, Vec<u32>>> = Vec::new();
    for term in query_terms {
        all_postings.push(read_postings(term, term_index, tier_idx).unwrap());
    }
    println!("  Phrase filtering {} candidates", final_list.len());
    let mut phrase_results: Vec<u32> = Vec::new();
    for doc_id in final_list {
        if has_phrase(doc_id, query_terms, &all_postings) {
            phrase_results.push(doc_id);
        }
    }
    phrase_results
}