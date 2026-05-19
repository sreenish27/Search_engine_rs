use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::Write;
use crate::encode_decode::{serialize_postings, BlockReader, Field};

// a function which does block by block processing and then merges everything and stored it in disk

//declare struct to declare a datatype for tiered postings
#[derive(Debug, Clone)]
pub struct TermEntry {
    pub offset:   u64,
    pub length:   u64,
    pub doc_freq: u32,
}

//a function to merge all the processed index_map blocks stored in disk as binary
//then send the index (term -> offset, length, doc_freq) to RAM and store posting lists contiguously in disk
pub fn merge_index_map() -> HashMap<String, TermEntry> {
    let num_blocks = fs::read_dir(".")
        .unwrap()
        .filter(|f| f.as_ref().unwrap().file_name().to_str().unwrap().starts_with("block_"))
        .count();
    println!("  Blocks found: {}", num_blocks);

    // create one reader per block, read first entry from each
    let mut readers: Vec<BlockReader> = Vec::new();
    let mut current: Vec<Option<(String, HashMap<u32, HashMap<Field, Vec<u32>>>)>> = Vec::new();
    for i in 1..=num_blocks {
        let mut reader = BlockReader::new(&format!("block_{}.bin", i));
        let entry = reader.next_entry();
        current.push(entry);
        readers.push(reader);
    }
    println!("  Block readers initialized");

    let mut postings_file = File::create("final_index.bin").unwrap();
    let mut offset: u64 = 0;
    let mut term_index: HashMap<String, TermEntry> = HashMap::new();
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
        let mut merged_postings: HashMap<u32, HashMap<Field, Vec<u32>>> = HashMap::new();
        for i in 0..readers.len() {
            if let Some((term, _)) = &current[i] {
                if term == &smallest {
                    let (_, postings) = current[i].take().unwrap();
                    for (doc_id, field_map) in postings {
                        debug_assert!(
                            !merged_postings.contains_key(&doc_id),
                            "duplicate doc_id {} for term '{}' across blocks", doc_id, smallest
                        );
                        merged_postings.insert(doc_id, field_map);
                    }
                    // advance this reader to its next entry
                    current[i] = readers[i].next_entry();
                }
            }
        }

        // write merged postings to disk in one contiguous blob
        let encoded = serialize_postings(&merged_postings);
        postings_file.write_all(&encoded).unwrap();

        // build RAM dictionary
        let length   = encoded.len() as u64;
        let doc_freq = merged_postings.len() as u32;
        term_index.insert(smallest, TermEntry { offset, length, doc_freq });
        offset += length;

        terms_merged += 1;
    }

    println!("  Terms merged: {}", terms_merged);
    println!("  Final index size: {} bytes", offset);

    term_index
}