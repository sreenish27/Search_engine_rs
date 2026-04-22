use std::fs;

//a class of function which read through content and remove extra characters and flatten everthing to alphanumeric (a lot of updates coming to this to capture all nuances!)

//accepts a file path - reads the bytes - checks for UTF-8 replaces where it is not, turns it into string and returns it
pub fn read_contents(file_path: &str) -> String {
    let content = fs::read(file_path).unwrap();
    String::from_utf8_lossy(&content).to_string()
}

//a function to take a document and split all terms and make a list
//also cleans up to include only alphanumeric and moves everything to lowercase
pub fn split_string(content: String) -> Vec<String> {
    content.split_whitespace().map(|word| word.to_string().to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()).collect()
}