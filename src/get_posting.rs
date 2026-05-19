use std::collections::HashMap;
use std::fs::File;
use std::io::{Seek, SeekFrom, Read};
use crate::block_merge::TermEntry;
use crate::encode_decode::{deserialize_postings, Field};

// a function which gets the postings for any given term from disk

//this function takes a term - gets term_index, gets offset and gets the posting stored in disk
//de-serializes and gets the values we want - used for phrase filtering
pub fn read_postings(
    term: &str,
    term_index: &HashMap<String, TermEntry>,
) -> Option<HashMap<u32, HashMap<Field, Vec<u32>>>> {
    let meta = term_index.get(term)?;
    let mut file = File::open("final_index.bin").unwrap();
    file.seek(SeekFrom::Start(meta.offset)).unwrap();
    let mut buffer = vec![0u8; meta.length as usize];
    file.read_exact(&mut buffer).unwrap();
    Some(deserialize_postings(&buffer))
}