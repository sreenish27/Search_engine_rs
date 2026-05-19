use std::collections::{HashMap, BTreeMap};
use std::fs;
use std::fs::File;
use std::io::Write;
use crate::encode_decode::{serialize_block, Field};
use crate::three_gram_index::three_gram_index;
use crate::cleanup::{read_contents, split_string};
use crate::field_extract::extract_fields;

pub struct DocStats {
    pub len_title:   u32,
    pub len_headers: u32,
    pub len_code:    u32,
    pub len_body:    u32,
}

//recursively traverses through a folder to get to all the files
//gets each file's path and processes the file contents into the inverted index
//added two variables to capture document length and - average document length (to help with BM25 calculations)
pub fn traverse(
    path: &str,
    index_map: &mut HashMap<String, HashMap<u32, HashMap<Field, Vec<u32>>>>,
    doc_id: &mut u32,
    doc_map: &mut HashMap<u32, String>,
    doc_stats: &mut HashMap<u32, DocStats>,
    gram_index: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), std::io::Error> {
    let entries = fs::read_dir(path)?;
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            traverse(
                entry.path().to_str().ok_or(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid UTF-8 path"))?,
                index_map, doc_id, doc_map, doc_stats, gram_index,
            )?;
        } else {
            //reducing the number of times i am calling the entry piece - to reduce the Pathbuf object allocation from 2 to 1
            let path_str = entry.path().to_str()
                .ok_or(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid UTF-8 path"))?
                .to_string();

            //block-based processing - every 4000 docs, write current index to disk in binary format and clear memory
            if *doc_id > 0 && *doc_id % 4000 == 0 {
                println!("  Writing block {} to disk, clearing memory", *doc_id / 4000);
                let encoded = serialize_block(&index_map);
                let filename = format!("block_{}.bin", *doc_id / 4000);
                let mut file = File::create(&filename)?;
                file.write_all(&encoded)?;
                //once written then clear index_map to free memory
                index_map.clear();
            }

            *doc_id += 1;
            //creating a map for docIDs and location of the files
            doc_map.insert(*doc_id, path_str.clone());

            let file_content = read_contents(&path_str);
            let fields = extract_fields(&file_content);

            // tokenize each field separately
            let title_tokens   = split_string(fields.title);
            let headers_tokens = split_string(fields.headers);
            let code_tokens    = split_string(fields.code);
            let body_tokens    = split_string(fields.body);

            // store per-field lengths for BM25F length normalization
            doc_stats.insert(*doc_id, DocStats {
                len_title:   title_tokens.len()   as u32,
                len_headers: headers_tokens.len() as u32,
                len_code:    code_tokens.len()     as u32,
                len_body:    body_tokens.len()     as u32,
            });

            let current_doc_id = *doc_id; // copy before closure borrows index_map and gram_index

            //code block to create the inverted index and also positional index
            //also builds trigram index for new terms (for spell correction)
            //positions restart at 0 per field — title pos 0 and body pos 0 are different coordinate spaces
            let mut index_field = |field: Field, tokens: &[String]| {
                for (pos, term) in tokens.iter().enumerate() {
                    if !index_map.contains_key(term) {
                        three_gram_index(term, gram_index);
                    }
                    index_map
                        .entry(term.to_string())
                        .or_insert_with(HashMap::new)
                        .entry(current_doc_id)
                        .or_insert_with(HashMap::new)
                        .entry(field)
                        .or_insert_with(Vec::new)
                        .push(pos as u32);
                }
            };

            index_field(Field::Title,   &title_tokens);
            index_field(Field::Headers, &headers_tokens);
            index_field(Field::Code,    &code_tokens);
            index_field(Field::Body,    &body_tokens);
        }
    }

    Ok(())
}