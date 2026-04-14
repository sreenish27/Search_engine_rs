use std::{collections::HashMap, fs};
use std::fs::File;
use std::io::Write;
use std::io;
use std::time::Instant;
use std::collections::{BTreeMap, HashSet};

use std::io::{Seek, SeekFrom, Read};

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
        let encoded = bincode::serialize(&index_map).unwrap();
        let block_num = (doc_id / 4000) + 1;
        let filename = format!("block_{}.bin", block_num);
        let mut file = File::create(&filename).unwrap();
        file.write_all(&encoded).unwrap();
        index_map.clear();
    }
    println!("  Documents processed: {}", doc_id);
    println!("  Unique trigrams in gram index: {}", gram_index.len());
    println!("  Index construction time: {:?}", t.elapsed());

    println!("--- MERGING BLOCKS ---");
    let t = Instant::now();
    //merge all block files into one final index on disk, return RAM dictionary
    let term_index = merge_index_map();
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
    let duration = start.elapsed();
    println!("Total search time: {:?}", duration);
}

//recursively traverses through a folder to get to all the files
//gets each file's path and processes the file contents into the inverted index
fn traverse(path: &str, index_map: &mut HashMap<String, HashMap<u32, Vec<u32>>>, doc_id: &mut u32, doc_map: &mut HashMap<u32, String>, gram_index: &mut BTreeMap<String, Vec<String>>) {
    let entries = fs::read_dir(path).unwrap();
    for entry in entries {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            traverse(entry.path().to_str().unwrap(), index_map, doc_id, doc_map, gram_index);
        } else {
            //block-based processing - every 4000 docs, write current index to disk in binary format and clear memory
            if *doc_id > 0 && *doc_id % 4000 == 0 {
                println!("  Writing block {} to disk, clearing memory", *doc_id / 4000);
                let encoded = bincode::serialize(&index_map).unwrap();
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

//accepts a file path - reads the bytes - checks for UTF-8 replaces where it is not, turns it into string and returns it
fn read_contents(file_path: &str) -> String {
    let content = fs::read(file_path).unwrap();
    String::from_utf8_lossy(&content).to_string()
}

//a function to take a document and split all terms and make a list
//also cleans up to include only alphanumeric and moves everything to lowercase
fn split_string(content: String) -> Vec<String> {
    content.split_whitespace().map(|word| word.to_string().to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()).collect()
}

//a function which gets the list of docIDs to intersect
//uses term_index (RAM dictionary) to get offsets, reads postings from disk
//sorts by doc_freq (smallest first) for optimal intersection order
fn docid_list(term_list: &Vec<String>, term_index: &HashMap<String, (u64, u64, u32)>) -> Vec<Vec<u32>> {
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
        let postings: HashMap<u32, Vec<u32>> = bincode::deserialize(&buffer).unwrap();
        let mut doc_ids: Vec<u32> = postings.keys().cloned().collect();
        doc_ids.sort();
        println!("    '{}': {} docs (read {} bytes from offset {})", term, doc_ids.len(), length, offset);
        posting_lists.push(doc_ids);
    }

    posting_lists
}

//a function which intersects between all word docID lists and gives final relevant documents
fn intersect_all(doc_id_list: Vec<Vec<u32>>) -> Vec<u32> {
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
fn intersect_two(list1: &Vec<u32>, list2: &Vec<u32>) -> Vec<u32> {
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

//checks if a phrase exists in a document by verifying consecutive positions
//term[0] at pos p, term[1] at p+1, term[2] at p+2, etc.
fn has_phrase(doc_id: u32, query_terms: &Vec<String>, all_postings: &Vec<HashMap<u32, Vec<u32>>>) -> bool {
    //get position list for the first term
    let first_positions = all_postings[0].get(&doc_id).unwrap();
    //for each starting position of the first term
    for &p in first_positions {
        let mut found = true;
        //check if every subsequent term exists at p+1, p+2, p+3...
        for (offset, _) in query_terms.iter().enumerate().skip(1) {
            let term_positions = all_postings[offset].get(&doc_id).unwrap();
            if term_positions.binary_search(&(p + offset as u32)).is_err() {
                found = false;
                break;
            }
        }
        if found {
            return true;
        }
    }
    false
}

//filters the doc list to only docs containing the exact phrase
//reads postings from disk once for all terms, then checks positional adjacency
fn phrase_filter(final_list: Vec<u32>, query_terms: &Vec<String>, term_index: &HashMap<String, (u64, u64, u32)>) -> Vec<u32> {
    //read all postings once from disk - don't read per document
    let mut all_postings: Vec<HashMap<u32, Vec<u32>>> = Vec::new();
    for term in query_terms {
        all_postings.push(read_postings(term, term_index).unwrap());
    }
    println!("  Phrase filtering {} candidates", final_list.len());
    let mut phrase_results: Vec<u32> = Vec::new();
    for doc_id in final_list {
        if has_phrase(doc_id, query_terms, &all_postings) {
            phrase_results.push(doc_id);
        }
    }
    phrase_results
}

//builds a 3-gram index for a term - breaks term into 3 character sequences with $ markers
//maps each trigram to the list of terms that contain it
fn three_gram_index(term: &str, gram_index: &mut BTreeMap<String, Vec<String>>) {
    let padded = format!("${}$", term);
    let mut i: usize = 0;
    while i < padded.len() - 2 {
        let gram = padded[i..i + 3].to_string();
        let term_list = gram_index.entry(gram).or_insert(Vec::new());
        if !term_list.contains(&term.to_string()) {
            term_list.push(term.to_string());
        }
        i += 1;
    }
}

//implementing my spell correction function - leverages trigram index to get possible words
//pipeline: trigram candidate lookup → Jaccard similarity filtering → Levenshtein edit distance ranking
fn spell_corrector(term: &str, tri_gram_index: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    //break the term into trigrams and put it in a list
    let mut trigram_list: Vec<String> = Vec::new();
    let mut i: usize = 0;
    let padded: String = format!("${}$", term);
    while i < padded.len() - 2 {
        trigram_list.push(padded[i..i + 3].to_string());
        i += 1;
    }
    //now get the trigrams list of terms from trigram_index and store it in a set
    let mut all_terms: HashSet<String> = HashSet::new();
    for trigram in &trigram_list {
        if let Some(terms) = tri_gram_index.get(trigram) {
            all_terms.extend(terms.iter().cloned());
        }
    }
    println!("  Spell check: '{}' → {} raw candidates from trigram index", term, all_terms.len());
    //now get the final candidates based on jaccard distance
    let mut candidates: Vec<(String, f64)> = Vec::new();
    for candidate in &all_terms {
        let score: f64 = jaccard_distance(term, candidate);
        if score > 0.3 {
            candidates.push((candidate.clone(), score));
        }
    }
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    println!("  Spell check: '{}' → {} after Jaccard filter", term, candidates.len());
    //now check for edit distance for these terms and then finally get the possible terms
    let mut possible_list: Vec<(String, usize)> = Vec::new();
    for (candidate, _score) in candidates.iter() {
        let e_distance: usize = edit_distance(term, candidate);
        if e_distance < term.len() / 3 {
            possible_list.push((candidate.clone(), e_distance));
        }
    }
    possible_list.sort_by_key(|a| a.1);
    println!("  Spell check: '{}' → {} after edit distance filter", term, possible_list.len());
    let final_list: Vec<String> = possible_list.iter().map(|(term, _)| term.clone()).collect();
    final_list
}

//jaccard distance = intersection / union of trigram sets between two terms
fn jaccard_distance(term1: &str, term2: &str) -> f64 {
    let grams1: HashSet<String> = three_gram_set(term1);
    let grams2: HashSet<String> = three_gram_set(term2);
    let intersection = grams1.intersection(&grams2).count();
    let union = grams1.union(&grams2).count();
    intersection as f64 / union as f64
}

//helper to get the set of trigrams for a term
fn three_gram_set(term: &str) -> HashSet<String> {
    let mut grams: HashSet<String> = HashSet::new();
    let padded = format!("${}$", term);
    let mut i: usize = 0;
    while i < padded.len() - 2 {
        grams.insert(padded[i..i + 3].to_string());
        i += 1;
    }
    grams
}

//edit distance (Levenshtein) - minimum insertions, deletions, substitutions to transform one string into another
//uses dynamic programming - O(m * n) where m and n are string lengths
fn edit_distance(term1: &str, term2: &str) -> usize {
    let a: Vec<char> = term1.chars().collect();
    let b: Vec<char> = term2.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut curr = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            if a[i - 1] == b[j - 1] {
                curr[j] = prev[j - 1];
            } else {
                curr[j] = 1 + prev[j].min(curr[j - 1]).min(prev[j - 1]);
            }
        }
        prev = curr;
    }
    prev[b.len()]
}

//a function to merge all the processed index_map blocks stored in disk as binary
//then send the index (term -> offset, length, doc_freq) to RAM and store posting lists contiguously in disk
fn merge_index_map() -> HashMap<String, (u64, u64, u32)> {
    //read the number of blocks in disk
    let num_blocks = fs::read_dir(".")
        .unwrap()
        .filter(|f| f.as_ref().unwrap().file_name().to_str().unwrap().starts_with("block_"))
        .count();
    println!("  Blocks found: {}", num_blocks);

    let mut blocks: Vec<HashMap<String, HashMap<u32, Vec<u32>>>> = Vec::new();
    for i in 1..=num_blocks {
        let data = fs::read(format!("block_{}.bin", i)).unwrap();
        let block = bincode::deserialize(&data).unwrap();
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
        //write merged postings to disk
        let encoded = bincode::serialize(&merged_postings).unwrap();
        postings.write_all(&encoded).unwrap();

        //write index to RAM - store offset, length, and doc_freq
        let length = encoded.len() as u64;
        let doc_freq = merged_postings.len() as u32;
        term_index.insert(term.clone(), (offset, length, doc_freq));

        //basically moves the offset to store the next term - once we know the length after merging from all blocks
        offset += length;
    }

    println!("  Final index size: {} bytes", offset);
    term_index
}

//this function takes a term - gets term_index, gets offset and gets the posting stored in disk
//de-serializes and gets the values we want - used for phrase filtering
fn read_postings(term: &str, term_index: &HashMap<String, (u64, u64, u32)>) -> Option<HashMap<u32, Vec<u32>>> {
    let meta = term_index.get(term)?;
    let mut file = File::open("final_index.bin").unwrap();
    file.seek(SeekFrom::Start(meta.0)).unwrap();
    let mut buffer = vec![0u8; meta.1 as usize];
    file.read_exact(&mut buffer).unwrap();
    Some(bincode::deserialize(&buffer).unwrap())
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