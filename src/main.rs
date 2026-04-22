use std::hash::Hash;
use std::{collections::HashMap, fs};
use std::fs::File;
use std::io::Write;
use std::io;
use std::time::Instant;
use std::collections::{BTreeMap, HashSet};

use std::io::{Seek, SeekFrom, Read};

//import all written libraries
mod traverse;
mod cleanup;
mod encode_decode;
mod block_merge;
mod get_posting;
mod intersect;
mod phrase_check;
mod spell_check;
mod three_gram_index;
mod tf_idf_index;

//specify the functions being used
use traverse::traverse;
use cleanup::{read_contents, split_string};
use encode_decode::{serialize_block, deserialize_block, serialize_postings, deserialize_postings, vbyte_encode, vbyte_decode};
use block_merge::merge_index_map;
use get_posting::read_postings;
use intersect::{intersect_all, docid_list};
use phrase_check::{phrase_filter, has_phrase};
use spell_check::{spell_corrector, jaccard_distance, three_gram_set, edit_distance};
use three_gram_index::three_gram_index;
use tf_idf_index::{tf_idf, rank_results};

fn main() {
    let total_start = Instant::now();
    let root = "/Users/krithik-qfit/Desktop/Search_engine/hello_cargo/20news-bydate/20news-bydate-train";
    //the inverted positional index hashmap - term -> {doc_id -> [positions]}
    let mut index_map: HashMap<String, HashMap<u32, Vec<u32>>> = HashMap::new();
    let mut doc_id: u32 = 0;
    //mapping doc_ids to file paths
    let mut doc_map: HashMap<u32, String> = HashMap::new();
    //3-gram index to take care of wildcard queries and spell correction
    let mut gram_index: BTreeMap<String, Vec<String>> = BTreeMap::new();

    println!("--- INDEX CONSTRUCTION ---");
    let t = Instant::now();
    traverse(root, &mut index_map, &mut doc_id, &mut doc_map, &mut gram_index);
    //to process the remaining docs that didn't hit the 4000 block checkpoint
    if !index_map.is_empty() {
        let encoded = serialize_block(&index_map);
        let block_num = (doc_id / 4000) + 1;
        let filename = format!("block_{}.bin", block_num);
        let mut file = File::create(&filename).unwrap();
        file.write_all(&encoded).unwrap();
        index_map.clear();
    }
    //store the total number of documents here
    let tot_docs:f32 = doc_id as f32;
    println!("  Documents processed: {}", doc_id);
    println!("  Unique trigrams in gram index: {}", gram_index.len());
    println!("  Index construction time: {:?}", t.elapsed());

    println!("--- MERGING BLOCKS ---");
    let t = Instant::now();
    //merge all block files into one final index on disk, return RAM dictionary
    let (term_index, doc_vec_len) = merge_index_map(tot_docs);
    println!("  Terms in final index: {}", term_index.len());
    println!("  Merge time: {:?}", t.elapsed());

    println!("--- READY FOR QUERIES ---");
    println!("  Total setup time: {:?}", total_start.elapsed());

    //make user give a search query and give docIDs which match
    //accept user query
    let mut query: String = String::new();
    println!("\nEnter your search query:");
    io::stdin().read_line(&mut query).unwrap();
    let start = Instant::now();
    let query = query.trim().to_lowercase().to_string();

    let mut query_list: Vec<String> = query.split_whitespace().map(|w| w.to_string()).collect();

    //run my spell checker algorithm - using K-gram before passing final stuff to search engine
    println!("--- SPELL CHECKING ---");
    let t = Instant::now();
    for i in 0..query_list.len() {
        if !term_index.contains_key(&query_list[i]) {
            let suggestions = spell_corrector(&query_list[i], &gram_index);
            if !suggestions.is_empty() {
                query_list[i] = suggestions[0].clone();
            }
        }
    }
    //to inform user im dropping stuff not there at all
    let before_len = query_list.len();
    query_list.retain(|term| {
        if term_index.contains_key(term) {
            true
        } else {
            println!("  Dropping '{}' — no match found in index", term);
            false
        }
    });
    if query_list.len() < before_len {
        println!("  Warning: {} term(s) dropped, results may be broader than intended", before_len - query_list.len());
    }

    let corrected_query: String = query_list.join(" ");
    println!("  Did you mean: \x1b[3m{}\x1b[0m?", corrected_query);
    println!("  Spell check time: {:?}", t.elapsed());

    println!("--- RETRIEVING POSTINGS ---");
    let t = Instant::now();
    let term_list = docid_list(&query_list, &term_index);
    println!("  Postings retrieval time: {:?}", t.elapsed());

    println!("--- INTERSECTING ---");
    let t = Instant::now();
    let results = intersect_all(term_list);
    println!("  Documents after intersection: {}", results.len());
    println!("  Intersection time: {:?}", t.elapsed());

    //phrase filter - checks positional adjacency for exact phrase matches
    println!("--- PHRASE FILTERING ---");
    let t = Instant::now();
    let results = phrase_filter(results, &query_list, &term_index);
    println!("  Documents after phrase filter: {}", results.len());
    println!("  Phrase filter time: {:?}", t.elapsed());

    println!("--- RESULTS ---");
    println!("{:?}", results);

    //rank the results
    let ranked_results = rank_results(results, &term_index, &query_list, tot_docs, &doc_vec_len);
    println!("--- RANKED RESULTS ---");
    println!("{:?}", ranked_results);

    

    let duration = start.elapsed();
    println!("Total search time: {:?}", duration);

    //get the location of all of them
    for (doc_id, score) in &ranked_results {
        let path = &doc_map[doc_id];
        println!("  Doc {}: {:.4} → {}", doc_id, score, path);
    }

}