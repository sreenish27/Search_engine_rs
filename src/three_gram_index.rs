use std::collections::BTreeMap;

//a function to create the 3-gram index

//builds a 3-gram index for a term - breaks term into 3 character sequences with $ markers
//maps each trigram to the list of terms that contain it
pub fn three_gram_index(term: &str, gram_index: &mut BTreeMap<String, Vec<String>>) {
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