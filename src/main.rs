// use std::hash::Hash;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::io;
use std::time::Instant;
use std::collections::BTreeMap;

//import all written libraries
mod traverse;
mod cleanup;
mod encode_decode;
mod block_merge;
mod get_posting;
mod intersect;
// mod phrase_check;
mod spell_check;
mod three_gram_index;
mod tf_idf_index;
mod field_extract;

//specify the functions being used
use traverse::{traverse, DocStats};
use encode_decode::{serialize_block, Field};
use block_merge::merge_index_map;
use intersect::{intersect_all, docid_list};
use spell_check::spell_corrector;
use tf_idf_index::{rank_results, compute_avg_lengths, BM25FParams};

fn main() {
    let total_start = Instant::now();
    let root = "/Users/krithik-qfit/Desktop/Search_engine/hello_cargo/corpus";

    //the inverted positional index hashmap - term -> {doc_id -> {field -> [positions]}}
    let mut index_map: HashMap<String, HashMap<u32, HashMap<Field, Vec<u32>>>> = HashMap::new();
    let mut doc_id: u32 = 0;
    //mapping doc_ids to file paths
    let mut doc_map: HashMap<u32, String> = HashMap::new();
    //per-doc field lengths — needed for BM25F length normalization
    let mut doc_stats: HashMap<u32, DocStats> = HashMap::new();
    //3-gram index to take care of wildcard queries and spell correction
    let mut gram_index: BTreeMap<String, Vec<String>> = BTreeMap::new();

    println!("--- INDEX CONSTRUCTION ---");
    let t = Instant::now();
    if let Err(e) = traverse(root, &mut index_map, &mut doc_id, &mut doc_map, &mut doc_stats, &mut gram_index) {
        eprintln!("Traversal failed: {}", e);
    }
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
    let tot_docs: f32 = doc_id as f32;
    println!("  Documents processed: {}", doc_id);
    println!("  Unique trigrams in gram index: {}", gram_index.len());
    println!("  Index construction time: {:?}", t.elapsed());

    println!("--- MERGING BLOCKS ---");
    let t = Instant::now();
    //merge all block files into one final index on disk, return RAM dictionary
    let term_index = merge_index_map();
    println!("  Terms in final index: {}", term_index.len());
    println!("  Merge time: {:?}", t.elapsed());

    //compute average field lengths once — query-independent, held in RAM for BM25F
    let avg_lengths = compute_avg_lengths(&doc_stats);
    //BM25F tuning parameters — weights and length normalization per field
    let bm25f_params = BM25FParams::default();
    println!("  Avg lengths — title: {:.1}  headers: {:.1}  code: {:.1}  body: {:.1}",
        avg_lengths.title, avg_lengths.headers, avg_lengths.code, avg_lengths.body);

    println!("--- READY FOR QUERIES ---");
    println!("  Total setup time: {:?}", total_start.elapsed());

    //query loop - keep accepting queries till user types "quit"
    loop {
        let mut query: String = String::new();
        println!("\nEnter your search query (or 'quit' to exit):");
        io::stdin().read_line(&mut query).unwrap();
        let query = query.trim().to_lowercase().to_string();
        if query == "quit" || query == "exit" || query.is_empty() {
            println!("Goodbye.");
            break;
        }

        let mut query_list: Vec<String> = query.split_whitespace().map(|w| w.to_string()).collect();

        //run my spell checker algorithm - using K-gram before passing final stuff to search engine
        println!("--- SPELL CHECKING ---");
        let t = Instant::now();
        for i in 0..query_list.len() {
            if !term_index.contains_key(&query_list[i]) {
                let suggestions = spell_corrector(&query_list[i], &gram_index, &term_index);
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

        let total_t = Instant::now();
        let k: usize = 10;

        println!("--- RETRIEVING POSTINGS ---");
        let t = Instant::now();
        let term_list = docid_list(&query_list, &term_index);
        println!("  Postings retrieval time: {:?}", t.elapsed());

        println!("--- INTERSECTING ---");
        let t = Instant::now();
        let results = intersect_all(term_list);
        println!("  Documents after intersection: {}", results.len());
        println!("  Intersection time: {:?}", t.elapsed());

        // //phrase filter - checks positional adjacency for exact phrase matches
        // // RETIRED May 1 — replaced by proximity scoring (omega_calc + boost_calc) in tf_idf_index.rs
        // // phrase_check.rs kept on disk for reference; mod declaration removed below
        // println!("--- PHRASE FILTERING ---");
        // let t = Instant::now();
        // let results = phrase_filter(results, &query_list, &term_index);
        // println!("  Documents after phrase filter: {}", results.len());
        // println!("  Phrase filter time: {:?}", t.elapsed());

        if results.is_empty() {
            println!("  No matching documents found.");
            continue;
        }

        println!("--- RANKING ---");
        let t = Instant::now();
        let mut ranked = rank_results(
            results,
            &query_list,
            &term_index,
            &doc_stats,
            &avg_lengths,
            &bm25f_params,
            tot_docs,
        );
        println!("  Ranking time: {:?}", t.elapsed());

        ranked.truncate(k);

        //IMPORTANT: rank_results already sorts descending — truncate keeps top K.
        //No re-sort needed here unlike the old tiered approach where scores from
        //different tiers needed a final merge sort across all tiers' results.
        println!("\n========================================");
        println!("  FINAL RANKED RESULTS (top {})", ranked.len());
        println!("========================================");
        for (rank, (doc_id, score)) in ranked.iter().enumerate() {
            let path = &doc_map[doc_id];
            println!("  #{:>2}  doc {:>5}  score={:.4}  →  {}", rank + 1, doc_id, score, path);
        }

        println!("\n  Total search time: {:?}", total_t.elapsed());
    }
}