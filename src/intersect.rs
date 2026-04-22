use std::collections::HashMap;
use std::fs::File;
use std::io::{Seek, SeekFrom, Read};
use crate::encode_decode::deserialize_postings;

//a class of functions which - intersects between 2 lists of doc_ids from different terms in the fastest way for minimal time and memory complexity

//a function which intersects between all word docID lists and gives final relevant documents
pub fn intersect_all(doc_id_list: Vec<Vec<u32>>) -> Vec<u32> {
    //a check to handle if the doclist is empty
    if doc_id_list.is_empty() {
        return vec![];
    }
    let mut final_list: Vec<u32> = doc_id_list[0].clone();
    let mut i: usize = 1;
    while i < doc_id_list.len() {
        final_list = intersect_two(&final_list, &doc_id_list[i]);
        i += 1;
    }
    final_list
}

//a function to just intersect 2 sorted lists using two-pointer merge - O(x + y)
pub fn intersect_two(list1: &Vec<u32>, list2: &Vec<u32>) -> Vec<u32> {
    let mut intersect_list: Vec<u32> = Vec::new();
    let mut i: usize = 0;
    let mut j: usize = 0;
    while i < list1.len() && j < list2.len() {
        if list1[i] == list2[j] {
            intersect_list.push(list1[i]);
            i += 1;
            j += 1;
        } else if list1[i] < list2[j] {
            i += 1;
        } else {
            j += 1;
        }
    }
    intersect_list
}

//a function which gets the list of docIDs to intersect
//uses term_index (RAM dictionary) to get offsets, reads postings from disk
//sorts by doc_freq (smallest first) for optimal intersection order
pub fn docid_list(term_list: &Vec<String>, term_index: &HashMap<String, (u64, u64, u32)>) -> Vec<Vec<u32>> {
    let mut term_meta: Vec<(&String, u64, u64, u32)> = Vec::new();
    //get the metadata for each query term from RAM dictionary
    for term in term_list {
        if let Some(meta_data) = term_index.get(term) {
            term_meta.push((term, meta_data.0, meta_data.1, meta_data.2));
        }
    }
    //sort by doc_freq, smallest first - to minimize intermediate results during intersection
    term_meta.sort_by_key(|t| t.3);

    //now use offset and length to read posting lists from disk
    println!("  Reading {} posting lists from disk", term_meta.len());
    let mut file = File::open("final_index.bin").unwrap();
    let mut posting_lists: Vec<Vec<u32>> = Vec::new();

    //this uses the offset in term_meta to seek into the disk file, reads the postings and deserializes
    //we are getting only doc_ids here (keys) - not positional index, that comes during phrase filtering
    for (term, offset, length, doc_freq) in &term_meta {
        file.seek(SeekFrom::Start(*offset)).unwrap();
        let mut buffer = vec![0u8; *length as usize];
        file.read_exact(&mut buffer).unwrap();
        let postings: HashMap<u32, Vec<u32>> = deserialize_postings(&buffer);
        let mut doc_ids: Vec<u32> = postings.keys().cloned().collect();
        doc_ids.sort();
        println!("    '{}': {} docs (read {} bytes from offset {})", term, doc_ids.len(), length, offset);
        posting_lists.push(doc_ids);
    }

    posting_lists
}

// //a function to just intersect 2 lists - also implement skip pointers within - to reduce no. of operations being done
// fn intersect_two(list1: &Vec<u32>, list2:&Vec<u32>) -> Vec<u32> {
//     let mut intersect_list:Vec<u32> = Vec::new();
//     let l1_inc: usize = (list1.len() as f64).sqrt() as usize;
//     let l2_inc: usize = (list2.len() as f64).sqrt() as usize;
//     let mut i:usize = 0;
//     let mut j:usize = 0;
//
//     while i < list1.len() && j < list2.len() {
//         if list1[i] == list2[j] {
//             intersect_list.push(list1[i]);
//             i += 1;
//             j += 1;
//         }
//         else if list1[i] < list2[j] {
//             if i+l1_inc < list1.len() && list1[i+l1_inc] <= list2[j] {
//                 i += l1_inc;
//             }
//             else {
//                 i += 1;
//             }
//         }
//         else {
//             if j+l2_inc < list2.len() && list2[j+l2_inc] <= list1[i] {
//                 j += l2_inc;
//             }
//             else{
//                 j += 1;
//             }
//         }
//     }
//
//     intersect_list
// }