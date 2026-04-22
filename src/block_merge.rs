use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::File;
use std::io::Write;
use crate::encode_decode::{serialize_postings, deserialize_block};
use crate::tf_idf_index::tf_idf;

// a function which does block by block processing and then merges everything and stored it in disk

//a function to merge all the processed index_map blocks stored in disk as binary
//then send the index (term -> offset, length, doc_freq) to RAM and store posting lists contiguously in disk
pub fn merge_index_map(tot_docs: f32) -> (HashMap<String, (u64, u64, u32)>, HashMap<u32, f32>) {
    //read the number of blocks in disk
    let num_blocks = fs::read_dir(".")
        .unwrap()
        .filter(|f| f.as_ref().unwrap().file_name().to_str().unwrap().starts_with("block_"))
        .count();
    println!("  Blocks found: {}", num_blocks);

    let mut blocks: Vec<HashMap<String, HashMap<u32, Vec<u32>>>> = Vec::new();
    for i in 1..=num_blocks {
        let data = fs::read(format!("block_{}.bin", i)).unwrap();
        let block = deserialize_block(&data);
        blocks.push(block);
    }
    println!("  Blocks loaded into memory");

    //get all unique terms - complete vocabulary across all blocks
    let mut all_terms: HashSet<String> = HashSet::new();
    for block in &blocks {
        for term in block.keys() {
            all_terms.insert(term.clone());
        }
    }
    //sort them alphabetically
    let mut sorted_terms: Vec<String> = all_terms.into_iter().collect();
    sorted_terms.sort();
    println!("  Unique terms to merge: {}", sorted_terms.len());

    //a binary file to store all my postings contiguously - this is the final index on disk
    let mut postings = File::create("final_index.bin").unwrap();
    let mut offset: u64 = 0;

    //now merge operation - iterate through each term and iterate through every single block
    //and simply get the value of it and keep merging it
    //order is not of concern as blocks are ordered in ascending doc_id order
    //variable for the RAM index - term -> (offset, length, doc_freq)
    let mut term_index: HashMap<String, (u64, u64, u32)> = HashMap::new();

    //a variable which will hold doc_id:vector_length - which is square root of sum of square of tf-idf values of each unique term 
    //in a document - this is used to normalize the tf_idf values computed - for sum of each term in a query computed per document - to basically - 
    //tackle the problem of long document too much t_f - managing that and increasing focus on a topic
    let mut doc_vec_len: HashMap<u32, f32> = HashMap::new();

    for term in &sorted_terms {
        //collect postings for this term from all blocks
        let mut merged_postings: HashMap<u32, Vec<u32>> = HashMap::new();
        for block in &blocks {
            if let Some(postings) = block.get(term) {
                for (doc_id, positions) in postings {
                    merged_postings.insert(*doc_id, positions.clone());
                }
            }
        }
        //before writing to disk create the code to fill the doc_vec_len variable
        let df = merged_postings.len() as f32;
        for (docid, postions) in &merged_postings {
            let score = tf_idf(postions.len() as f32, tot_docs, df);
            *doc_vec_len.entry(*docid).or_insert(0.0) += score * score;
        }

        //write merged postings to disk
        let encoded = serialize_postings(&merged_postings);
        postings.write_all(&encoded).unwrap();

        //write index to RAM - store offset, length, and doc_freq
        let length = encoded.len() as u64;
        let doc_freq = merged_postings.len() as u32;
        term_index.insert(term.clone(), (offset, length, doc_freq));

        //basically moves the offset to store the next term - once we know the length after merging from all blocks
        offset += length;
    }

    //take sqrt of all vec_length values for each doc_id - this will give vector length
    for val in doc_vec_len.values_mut() {
        *val = val.sqrt();
    }

    println!("  Final index size: {} bytes", offset);

    (term_index, doc_vec_len)
}