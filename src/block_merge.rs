use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::Write;
use crate::encode_decode::{serialize_postings, BlockReader};
use crate::tf_idf_index::tf_idf;
// a function which does block by block processing and then merges everything and stored it in disk

//a function to merge all the processed index_map blocks stored in disk as binary
//then send the index (term -> offset, length, doc_freq) to RAM and store posting lists contiguously in disk
pub fn merge_index_map(tot_docs: f32) -> (HashMap<String, (u64, u64, u32)>, HashMap<u32, f32>) {
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
    let mut offset: u64 = 0;
    let mut term_index: HashMap<String, (u64, u64, u32)> = HashMap::new();
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

        // compute doc vector lengths
        let df = merged_postings.len() as f32;
        for (doc_id, positions) in &merged_postings {
            let score = tf_idf(positions.len() as f32, tot_docs, df);
            *doc_vec_len.entry(*doc_id).or_insert(0.0) += score * score;
        }

        // write merged postings to disk
        let encoded = serialize_postings(&merged_postings);
        postings_file.write_all(&encoded).unwrap();

        // build RAM dictionary
        let length = encoded.len() as u64;
        let doc_freq = merged_postings.len() as u32;
        term_index.insert(smallest, (offset, length, doc_freq));
        offset += length;

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