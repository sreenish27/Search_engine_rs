use std::{collections::HashMap, fs};

//a class of functions which takes data and serializes into contiguous bytes and de-serializes back into data more effectively than rust library

//a class for blockreader
#[derive(Debug, Clone)]
pub struct BlockReader {
    data: Vec<u8>,
    offset:usize,
    remaining:u32,
}

impl BlockReader {
    pub fn new(filename: &str) -> Self {
        let data = fs::read(filename).unwrap();
        let (term_count, bytes_read) = vbyte_decode(&data);
        BlockReader {
            data,
            offset: bytes_read,
            remaining: term_count,
        }
    }

    pub fn next_entry(&mut self) -> Option<(String, HashMap<u32, Vec<u32>>)> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
    
        // everything below is copied from your deserialize_block loop
        let (term_len, bytes_read) = vbyte_decode(&self.data[self.offset..]);
        self.offset += bytes_read;
    
        let term = String::from_utf8(self.data[self.offset..self.offset + term_len as usize].to_vec())
            .expect("invalid utf-8");
        self.offset += term_len as usize;
    
        let (postings_len, bytes_read) = vbyte_decode(&self.data[self.offset..]);
        self.offset += bytes_read;
    
        let postings = deserialize_postings(&self.data[self.offset..self.offset + postings_len as usize]);
        self.offset += postings_len as usize;
    
        Some((term, postings))
    }
}

//a function to basically - encode my content to binary - but being clear about size using gap encoding
pub fn vbyte_encode(mut n: u32, out: &mut Vec<u8>) {
    let mut tmp = [0u8; 5];
    let mut len = 0;
    loop {
        tmp[len] = (n & 0x7F) as u8;
        n >>= 7;
        len += 1;
        if n == 0 { break; }
    }
    for i in (1..len).rev() {
        out.push(tmp[i]);
    }
    out.push(tmp[0] | 0x80);
}

//to decode the encoded stuff
pub fn vbyte_decode(data: &[u8]) -> (u32, usize) {
    let mut result: u32 = 0;
    for (i, &byte) in data.iter().enumerate() {
        if byte & 0x80 != 0 {
            result = (result << 7) | (byte & 0x7F) as u32;
            return (result, i + 1);
        }
        result = (result << 7) | (byte & 0x7F) as u32;
    }
    panic!("unterminated vbyte");
}
//this takes the encode and does serialize on my postings basically
pub fn serialize_postings(postings: &HashMap<u32, Vec<u32>>) -> Vec<u8> {
    let mut out = Vec::new();

    // sort doc IDs so gaps are always positive
    let mut doc_ids: Vec<u32> = postings.keys().copied().collect();
    doc_ids.sort();

    // write doc count
    vbyte_encode(doc_ids.len() as u32, &mut out);

    let mut prev_doc: u32 = 0;
    for &doc_id in &doc_ids {
        // write doc ID gap (first doc writes full ID since prev_doc = 0)
        vbyte_encode(doc_id - prev_doc, &mut out); //this is the gap encoding - where gaps between doc_ids is calcualted
        prev_doc = doc_id;

        let positions = &postings[&doc_id];

        // write position count
        vbyte_encode(positions.len() as u32, &mut out);

        // write position gaps
        let mut prev_pos: u32 = 0;
        for &pos in positions {
            vbyte_encode(pos - prev_pos, &mut out);
            prev_pos = pos;
        }
    }

    out
}
//this decodes it
pub fn deserialize_postings(data: &[u8]) -> HashMap<u32, Vec<u32>> {
    let mut postings = HashMap::new();
    let mut offset = 0;

    // read doc count
    let (doc_count, bytes_read) = vbyte_decode(&data[offset..]);
    offset += bytes_read;

    let mut prev_doc: u32 = 0;
    for _ in 0..doc_count {
        // read doc ID gap, reconstruct absolute ID
        let (gap, bytes_read) = vbyte_decode(&data[offset..]);
        offset += bytes_read;
        let doc_id = prev_doc + gap;
        prev_doc = doc_id;

        // read position count
        let (pos_count, bytes_read) = vbyte_decode(&data[offset..]);
        offset += bytes_read;

        // read position gaps, reconstruct absolute positions
        let mut positions = Vec::with_capacity(pos_count as usize);
        let mut prev_pos: u32 = 0;
        for _ in 0..pos_count {
            let (gap, bytes_read) = vbyte_decode(&data[offset..]);
            offset += bytes_read;
            let pos = prev_pos + gap;
            positions.push(pos);
            prev_pos = pos;
        }

        postings.insert(doc_id, positions);
    }

    postings
}

pub fn serialize_block(index_map: &HashMap<String, HashMap<u32, Vec<u32>>>) -> Vec<u8> {
    let mut out = Vec::new();

    // sort terms alphabetically for consistent ordering and merge-friendly reads
    let mut terms: Vec<&String> = index_map.keys().collect();
    terms.sort();

    // write term count
    vbyte_encode(terms.len() as u32, &mut out);

    for term in &terms {
        let term_bytes = term.as_bytes();

        // write term length + raw term bytes
        vbyte_encode(term_bytes.len() as u32, &mut out);
        out.extend_from_slice(term_bytes);

        // serialize this term's postings
        let postings_bytes = serialize_postings(&index_map[*term]);

        // write postings length + postings bytes
        vbyte_encode(postings_bytes.len() as u32, &mut out);
        out.extend_from_slice(&postings_bytes);
    }

    out
}

pub fn deserialize_block(data: &[u8]) -> HashMap<String, HashMap<u32, Vec<u32>>> {
    let mut block = HashMap::new();
    let mut offset = 0;

    // read term count
    let (term_count, bytes_read) = vbyte_decode(&data[offset..]);
    offset += bytes_read;

    for _ in 0..term_count {
        // read term length + raw term bytes
        let (term_len, bytes_read) = vbyte_decode(&data[offset..]);
        offset += bytes_read;

        let term = String::from_utf8(data[offset..offset + term_len as usize].to_vec())
            .expect("invalid utf-8 in term");
        offset += term_len as usize;

        // read postings length + postings bytes
        let (postings_len, bytes_read) = vbyte_decode(&data[offset..]);
        offset += bytes_read;

        let postings = deserialize_postings(&data[offset..offset + postings_len as usize]);
        offset += postings_len as usize;

        block.insert(term, postings);
    }

    block
}