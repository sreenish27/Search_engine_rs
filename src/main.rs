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
    let mut index_map: HashMap<String, HashMap<u32, Vec<u32>>> = HashMap::new();
    let mut doc_id: u32 = 0;
    let mut doc_map: HashMap<u32, String> = HashMap::new();
    let mut gram_index: BTreeMap<String, Vec<String>> = BTreeMap::new();

    println!("--- INDEX CONSTRUCTION ---");
    let t = Instant::now();
    traverse(root, &mut index_map, &mut doc_id, &mut doc_map, &mut gram_index);
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
    let term_index = merge_index_map();
    println!("  Terms in final index: {}", term_index.len());
    println!("  Merge time: {:?}", t.elapsed());

    println!("--- READY FOR QUERIES ---");
    println!("  Total setup time: {:?}", total_start.elapsed());

    let mut query: String = String::new();
    println!("\nEnter your search query:");
    io::stdin().read_line(&mut query).unwrap();
    let start = Instant::now();
    let query = query.trim().to_lowercase().to_string();

    let mut query_list: Vec<String> = query.split_whitespace().map(|w| w.to_string()).collect();

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

fn traverse(path: &str, index_map: &mut HashMap<String, HashMap<u32, Vec<u32>>>, doc_id: &mut u32, doc_map: &mut HashMap<u32, String>, gram_index: &mut BTreeMap<String, Vec<String>>) {
    let entries = fs::read_dir(path).unwrap();
    for entry in entries {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            traverse(entry.path().to_str().unwrap(), index_map, doc_id, doc_map, gram_index);
        } else {
            if *doc_id > 0 && *doc_id % 4000 == 0 {
                println!("  Writing block {} to disk, clearing memory", *doc_id / 4000);
                let encoded = bincode::serialize(&index_map).unwrap();
                let filename = format!("block_{}.bin", *doc_id / 4000);
                let mut file = File::create(&filename).unwrap();
                file.write_all(&encoded).unwrap();
                index_map.clear();
            }
            *doc_id += 1;
            doc_map.insert(*doc_id, entry.path().to_str().unwrap().to_string());
            let file_content = read_contents(entry.path().to_str().unwrap());
            let terms: Vec<String> = split_string(file_content);
            for (pos, term) in terms.iter().enumerate() {
                if !index_map.contains_key(term) {
                    three_gram_index(term, gram_index);
                }
                index_map.entry(term.to_string()).or_insert(HashMap::new()).entry(*doc_id).or_insert(Vec::new()).push(pos as u32);
            }
        }
    }
}

fn read_contents(file_path: &str) -> String {
    let content = fs::read(file_path).unwrap();
    String::from_utf8_lossy(&content).to_string()
}

fn split_string(content: String) -> Vec<String> {
    content.split_whitespace().map(|word| word.to_string().to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()).collect()
}

fn docid_list(term_list: &Vec<String>, term_index: &HashMap<String, (u64, u64, u32)>) -> Vec<Vec<u32>> {
    let mut term_meta: Vec<(&String, u64, u64, u32)> = Vec::new();
    for term in term_list {
        if let Some(meta_data) = term_index.get(term) {
            term_meta.push((term, meta_data.0, meta_data.1, meta_data.2));
        }
    }
    term_meta.sort_by_key(|t| t.3);

    println!("  Reading {} posting lists from disk", term_meta.len());
    let mut file = File::open("final_index.bin").unwrap();
    let mut posting_lists: Vec<Vec<u32>> = Vec::new();

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

fn intersect_all(doc_id_list: Vec<Vec<u32>>) -> Vec<u32> {
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

fn has_phrase(doc_id: u32, query_terms: &Vec<String>, all_postings: &Vec<HashMap<u32, Vec<u32>>>) -> bool {
    let first_positions = all_postings[0].get(&doc_id).unwrap();
    for &p in first_positions {
        let mut found = true;
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

fn phrase_filter(final_list: Vec<u32>, query_terms: &Vec<String>, term_index: &HashMap<String, (u64, u64, u32)>) -> Vec<u32> {
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

fn spell_corrector(term: &str, tri_gram_index: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let mut trigram_list: Vec<String> = Vec::new();
    let mut i: usize = 0;
    let padded: String = format!("${}$", term);
    while i < padded.len() - 2 {
        trigram_list.push(padded[i..i + 3].to_string());
        i += 1;
    }
    let mut all_terms: HashSet<String> = HashSet::new();
    for trigram in &trigram_list {
        if let Some(terms) = tri_gram_index.get(trigram) {
            all_terms.extend(terms.iter().cloned());
        }
    }
    println!("  Spell check: '{}' → {} raw candidates from trigram index", term, all_terms.len());
    let mut candidates: Vec<(String, f64)> = Vec::new();
    for candidate in &all_terms {
        let score: f64 = jaccard_distance(term, candidate);
        if score > 0.3 {
            candidates.push((candidate.clone(), score));
        }
    }
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    println!("  Spell check: '{}' → {} after Jaccard filter", term, candidates.len());
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

fn jaccard_distance(term1: &str, term2: &str) -> f64 {
    let grams1: HashSet<String> = three_gram_set(term1);
    let grams2: HashSet<String> = three_gram_set(term2);
    let intersection = grams1.intersection(&grams2).count();
    let union = grams1.union(&grams2).count();
    intersection as f64 / union as f64
}

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

fn merge_index_map() -> HashMap<String, (u64, u64, u32)> {
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

    let mut all_terms: HashSet<String> = HashSet::new();
    for block in &blocks {
        for term in block.keys() {
            all_terms.insert(term.clone());
        }
    }
    let mut sorted_terms: Vec<String> = all_terms.into_iter().collect();
    sorted_terms.sort();
    println!("  Unique terms to merge: {}", sorted_terms.len());

    let mut postings = File::create("final_index.bin").unwrap();
    let mut offset: u64 = 0;
    let mut term_index: HashMap<String, (u64, u64, u32)> = HashMap::new();

    for term in &sorted_terms {
        let mut merged_postings: HashMap<u32, Vec<u32>> = HashMap::new();
        for block in &blocks {
            if let Some(postings) = block.get(term) {
                for (doc_id, positions) in postings {
                    merged_postings.insert(*doc_id, positions.clone());
                }
            }
        }
        let encoded = bincode::serialize(&merged_postings).unwrap();
        postings.write_all(&encoded).unwrap();
        let length = encoded.len() as u64;
        let doc_freq = merged_postings.len() as u32;
        term_index.insert(term.clone(), (offset, length, doc_freq));
        offset += length;
    }

    println!("  Final index size: {} bytes", offset);
    term_index
}

fn read_postings(term: &str, term_index: &HashMap<String, (u64, u64, u32)>) -> Option<HashMap<u32, Vec<u32>>> {
    let meta = term_index.get(term)?;
    let mut file = File::open("final_index.bin").unwrap();
    file.seek(SeekFrom::Start(meta.0)).unwrap();
    let mut buffer = vec![0u8; meta.1 as usize];
    file.read_exact(&mut buffer).unwrap();
    Some(bincode::deserialize(&buffer).unwrap())
}