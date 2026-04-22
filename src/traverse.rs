use std::collections::{HashMap, BTreeMap};
use std::fs;
use std::fs::File;
use std::io::Write;
use crate::encode_decode::serialize_block;
use crate::three_gram_index::three_gram_index;
use crate::cleanup::{read_contents, split_string};


//recursively traverses through a folder to get to all the files
//gets each file's path and processes the file contents into the inverted index
pub fn traverse(path: &str, index_map: &mut HashMap<String, HashMap<u32, Vec<u32>>>, doc_id: &mut u32, doc_map: &mut HashMap<u32, String>, gram_index: &mut BTreeMap<String, Vec<String>>) {
    let entries = fs::read_dir(path).unwrap();
    for entry in entries {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            traverse(entry.path().to_str().unwrap(), index_map, doc_id, doc_map, gram_index);
        } else {
            //block-based processing - every 4000 docs, write current index to disk in binary format and clear memory
            if *doc_id > 0 && *doc_id % 4000 == 0 {
                println!("  Writing block {} to disk, clearing memory", *doc_id / 4000);
                let encoded = serialize_block(&index_map);
                let filename = format!("block_{}.bin", *doc_id / 4000);
                let mut file = File::create(&filename).unwrap();
                file.write_all(&encoded).unwrap();
                //once written then clear index_map to free memory
                index_map.clear();
            }
            *doc_id += 1;
            //creating a map for docIDs and location of the files
            doc_map.insert(*doc_id, entry.path().to_str().unwrap().to_string());
            let file_content = read_contents(entry.path().to_str().unwrap());
            let terms: Vec<String> = split_string(file_content);
            //code block to create the inverted index and also positional index
            //also builds trigram index for new terms (for spell correction)
            for (pos, term) in terms.iter().enumerate() {
                if !index_map.contains_key(term) {
                    three_gram_index(term, gram_index);
                }
                index_map.entry(term.to_string()).or_insert(HashMap::new()).entry(*doc_id).or_insert(Vec::new()).push(pos as u32);
            }
        }
    }
}