use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::Write;
use crate::encode_decode::{serialize_postings, BlockReader};
use crate::tf_idf_index::tf_idf;
// a function which does block by block processing and then merges everything and stored it in disk

const NO_OF_TIERS: usize = 3;

//declare struct to declare a datatype for tiered postings
#[derive(Debug, Clone)]
pub struct TermEntry {
    pub tiers: [(u64, u64); NO_OF_TIERS],
    pub doc_freq: u32,
}

//a function to merge all the processed index_map blocks stored in disk as binary
//then send the index (term -> offset, length, doc_freq) to RAM and store posting lists contiguously in disk
pub fn merge_index_map(tot_docs: f32) -> (HashMap<String, TermEntry>, HashMap<u32, f32>) {
    //the tier splitting t_f(term frequency thresholds) - hyper parameters - subject to change based on testing
    let tier1: u32 = 5;
    let tier2: u32 = 2;

    let num_blocks = fs::read_dir(".")
        .unwrap()
        .filter(|f| f.as_ref().unwrap().file_name().to_str().unwrap().starts_with("block_"))
        .count();
    println!("  Blocks found: {}", num_blocks);

    // create one reader per block, read first entry from each
    let mut readers: Vec<BlockReader> = Vec::new();
    let mut current: Vec<Option<(String, HashMap<u32, Vec<u32>>)>> = Vec::new();
    for i in 1..=num_blocks {
        let mut reader = BlockReader::new(&format!("block_{}.bin", i));
        let entry = reader.next_entry();
        current.push(entry);
        readers.push(reader);
    }
    println!("  Block readers initialized");

    let mut postings_file = File::create("final_index.bin").unwrap();

    //to get distribution - and fix tier numbers in a rigorous way
    let mut tf_dump = File::create("tf_dump.csv").unwrap();
    writeln!(tf_dump, "term,doc_id,tf").unwrap();

    let mut offset: u64 = 0;
    let mut term_index: HashMap<String, TermEntry> = HashMap::new();
    let mut doc_vec_len: HashMap<u32, f32> = HashMap::new();
    let mut terms_merged: u32 = 0;

    loop {
        // find the smallest term across all current entries
        let mut smallest: Option<String> = None;
        for entry in &current {
            if let Some((term, _)) = entry {
                if smallest.is_none() || term < smallest.as_ref().unwrap() {
                    smallest = Some(term.clone());
                }
            }
        }

        // if no smallest found, all readers are exhausted
        let smallest = match smallest {
            Some(t) => t,
            None => break,
        };

        // collect postings for this term from all readers that have it
        let mut merged_postings: HashMap<u32, Vec<u32>> = HashMap::new();
        for i in 0..readers.len() {
            if let Some((term, _)) = &current[i] {
                if term == &smallest {
                    let (_, postings) = current[i].take().unwrap();
                    for (doc_id, positions) in postings {
                        merged_postings.insert(doc_id, positions);
                    }
                    // advance this reader to its next entry
                    current[i] = readers[i].next_entry();
                }
            }
        }

        // // compute doc vector lengths
        // let df = merged_postings.len() as f32;
        // for (doc_id, positions) in &merged_postings {
        //     let score = tf_idf(positions.len() as f32, tot_docs, df);
        //     *doc_vec_len.entry(*doc_id).or_insert(0.0) += score * score;
        // }

        //tier based splitting by looking at tf and enforcing hard thershold rules (will be updated as I look at results)
        // compute doc vector lengths

        //declare teach tier postings as well (subject to change as we add more tiers) - must stay inside the term loop (fresh variables for each term)
        let mut tier1_posting: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut tier2_posting: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut tier3_posting: HashMap<u32, Vec<u32>> = HashMap::new();
        
        let doc_freq = merged_postings.len() as u32;
        let df = merged_postings.len() as f32;
        for (doc_id, positions) in merged_postings {
            let score = tf_idf(positions.len() as f32, tot_docs, df);
            *doc_vec_len.entry(doc_id).or_insert(0.0) += score * score;
            //create csv for tiered - distributio
            writeln!(tf_dump, "{},{},{}", smallest, doc_id, positions.len()).unwrap();
            //a loop runs for all tiers
            if positions.len() as u32 > tier1 {
                tier1_posting.insert(doc_id, positions);
            }
            else if positions.len() as u32 > tier2 {
                tier2_posting.insert(doc_id, positions);
            }
            else {
                tier3_posting.insert(doc_id, positions);
            }
        }

        //add all the tiered postings to a vec
        let tiers = [tier1_posting, tier2_posting, tier3_posting];

        // println!("  '{}': t1={}, t2={}, t3={} (df={})", smallest, tiers[0].len(), tiers[1].len(), tiers[2].len(), doc_freq);

        // write merged postings to disk - now it is tiered so loop through it - and build RAM simultaneously for each term
        //a buffer to hold all three tuples for 3 tiers
        let mut tier_buffer: [(u64, u64); NO_OF_TIERS] = [(0, 0); NO_OF_TIERS];
        for i in 0..NO_OF_TIERS {
            let encoded = serialize_postings(&tiers[i]);
            postings_file.write_all(&encoded).unwrap();
            // build RAM dictionary
            let length = encoded.len() as u64;
            //add the len and offset to the buffer
            tier_buffer[i] = (offset, length);
            offset += length;
        }

        //now insert this into term index
        term_index.insert(smallest, TermEntry { tiers: tier_buffer, doc_freq });

        // let encoded = serialize_postings(&merged_postings);
        // postings_file.write_all(&encoded).unwrap();

        // // build RAM dictionary
        // let length = encoded.len() as u64;
        // let doc_freq = merged_postings.len() as u32;
        // term_index.insert(smallest, (offset, length, doc_freq));
        // offset += length;

        terms_merged += 1;
    }

    // take sqrt of all doc vector lengths
    for val in doc_vec_len.values_mut() {
        *val = val.sqrt();
    }

    println!("  Terms merged: {}", terms_merged);
    println!("  Final index size: {} bytes", offset);

    (term_index, doc_vec_len)
}