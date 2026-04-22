use std::collections::HashMap;
use crate::get_posting::read_postings;

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

//a ranking function which uses tf - idf 
pub fn rank_results(results: Vec<u32>, term_index: &HashMap<String, (u64, u64, u32)>, terms: &Vec<String>, total_docs:f32, doc_vec_len: &HashMap<u32, f32>) -> Vec<(u32, f32)> {
    let mut all_postings: HashMap<String, HashMap<u32, Vec<u32>>> = HashMap::new();
    //get all required postings for each term and store - only once we retrieve and deserialize
    for term in terms {
        let posting = read_postings(&term, term_index).unwrap();
        all_postings.insert(term.clone(), posting);
    }

    //now do the rest
    //declare a variable to hold doc_id and final tf_idf score
    let mut ranked_docs: Vec<(u32, f32)> = Vec::new();
    for &doc_id in &results {
        let mut doc_score:f32 = 0.0;
        for term in terms {
            let posting = &all_postings[term];
            let t_f = posting[&doc_id].len() as f32;
            let d_f = posting.len() as f32;
            let score = tf_idf(t_f, total_docs, d_f);
            doc_score += score;
        }
        //before pushing - once I am done for a doc_id all terms - divide by the vector length for 
        //that doc_id which is already pre-computed and stored
        doc_score /= doc_vec_len[&doc_id];
        ranked_docs.push((doc_id, doc_score));
    }
    //sort them before giving the result
    ranked_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    //final result
    ranked_docs
}