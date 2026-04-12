use std::{collections::HashMap, fs};
use std::fs::File;
use std::io::Write;
use std::io;
use std::time::Instant;

fn main() {
    let root = "/Users/krithik-qfit/Desktop/hello_world/hello_cargo/20news-bydate/20news-bydate-train";
    let mut index_map: HashMap<String, HashMap<u32, Vec<u32>>> = HashMap::new();
    let mut doc_id: u32 = 0;
    let mut doc_map: HashMap<u32, String> = HashMap::new();
    traverse(root, &mut index_map, &mut doc_id, &mut doc_map);
    //my inverted index hashmap
    // println!("{:?}", index_map);
    // println!("{:?}", doc_map);
    let mut sorted_docs: HashMap<String, Vec<u32>> = HashMap::new();
    for (term, doc_hash) in &index_map {
        let mut keys: Vec<u32> = doc_hash.keys().cloned().collect();
        keys.sort();
        sorted_docs.insert(term.clone(), keys);
}
    let json = serde_json::to_string_pretty(&index_map).unwrap();
    let mut file = File::create("index.json").unwrap();
    file.write_all(json.as_bytes()).unwrap();

    //make user give a search query and give docIDs which match
    //accept user query
    let mut query:String = String::new();
    println!("{}", "Enter your search query:");
    io::stdin().read_line(&mut query).unwrap();
    let start = Instant::now();
    let query = query.trim().to_lowercase().to_string();

    let query_list: Vec<String> = query.split_whitespace().map(|w| w.to_string()).collect();

    let term_list = docid_list(query_list, &sorted_docs);
    let results = intersect_all(term_list);

    println!("{:?}", results);
    let duration = start.elapsed();
    println!("Search took: {:?}", duration);

}

//recrusively traverses through a folder to get to all the files 
//- gets each files path and prints the file path and file contents
fn traverse(path:&str, index_map: &mut HashMap<String, HashMap<u32, Vec<u32>>>, doc_id: &mut u32, doc_map: &mut HashMap<u32, String>) {
    let entries = fs::read_dir(path).unwrap();
    //the inverted document index hashmap
    //for document ID
    for entry in entries {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            traverse(entry.path().to_str().unwrap(), index_map, doc_id, doc_map);
        }
        else {
            *doc_id += 1;
            //creating a map for docIDs and location of the files
            doc_map.insert(*doc_id, entry.path().to_str().unwrap().to_string());
            let file_content = read_contents(entry.path().to_str().unwrap());
            // println!("{:?}",split_string(file_content));
            let terms: Vec<String> = split_string(file_content);
            //code block to create the inverted index and also positional index code
            for (pos, term) in terms.iter().enumerate() {
                index_map.entry(term.to_string()).or_insert(HashMap::new()).entry(*doc_id).or_insert(Vec::new()).push(pos as u32);
            }
        }
    }
}

//accepts a file path - reads the bytes - checks for UTF-8 replaces where it is not turns is into string and returns it
fn read_contents (file_path:&str) -> String {
    let content = fs::read(file_path).unwrap();
    let text = String::from_utf8_lossy(&content).to_string();
    text
}

//a function to take a document and split all terms and make a list - also cleans up to include only alphanumeric and moves everything to lowercase
fn split_string(content: String) -> Vec<String> {
    let content_list = content.split_whitespace().map(|word| word.to_string().to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()).collect();
    content_list
}

//a function which gets the list of docIDs to intersect
fn docid_list(term_list: Vec<String>, sorted_docs: &HashMap<String, Vec<u32>>) -> Vec<Vec<u32>> {
    let mut temp_list: Vec<Vec<u32>> = Vec::new();
    //get the docids and positional index list
    for term in term_list {
        let doc_ids = sorted_docs.get(&term);
        if doc_ids.is_none() {
            return Vec::new();
        }
        temp_list.push(doc_ids.unwrap().clone());
    }
    //sort based on length of docids for each term
    temp_list.sort_by_key(|list| list.len());

    temp_list
}

//a function which intersects between all word docID lists and gives final relevant documents
fn intersect_all(doc_id_list: Vec<Vec<u32>>) -> Vec<u32> {
    //a check to handle if the doclist is empty
    if doc_id_list.is_empty(){
        return Vec::new();
    }
    let mut final_list: Vec<u32> = doc_id_list[0].clone();
    let mut i: usize = 1;
    while i < doc_id_list.len() {
        final_list = intersect_two(&final_list, &doc_id_list[i]);
        i += 1;
    }

    final_list
}

//a function to just intersect 2 lists
fn intersect_two(list1: &Vec<u32>, list2:&Vec<u32>) -> Vec<u32> {
    let mut intersect_list:Vec<u32> = Vec::new();
    let mut i:usize = 0;
    let mut j:usize = 0;

    while i < list1.len() && j < list2.len() {
        if list1[i] == list2[j] {
            intersect_list.push(list1[i]);
            i += 1;
            j += 1;
        }
        else if list1[i] < list2[j] {
            i += 1;
        }
        else {
            j += 1;
        }
    }

    intersect_list
}