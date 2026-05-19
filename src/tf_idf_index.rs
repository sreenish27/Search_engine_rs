use std::collections::HashMap;
use std::time::Instant;
use crate::block_merge::TermEntry;
use crate::get_posting::read_postings;
use crate::encode_decode::Field;
use crate::traverse::DocStats;

// Toggle diagnostic prints on/off. Set to false to measure pure scoring time.
const VERBOSE: bool = true;

//a class of functions to calculate BM25F scores and rank the search results using it

pub struct AvgLengths {
    pub title:   f32,
    pub headers: f32,
    pub code:    f32,
    pub body:    f32,
}

pub fn compute_avg_lengths(doc_stats: &HashMap<u32, DocStats>) -> AvgLengths {
    debug_assert!(!doc_stats.is_empty(), "compute_avg_lengths called with empty doc_stats");

    let n = doc_stats.len() as f64;

    let mut sum_title:   f64 = 0.0;
    let mut sum_headers: f64 = 0.0;
    let mut sum_code:    f64 = 0.0;
    let mut sum_body:    f64 = 0.0;

    for stats in doc_stats.values() {
        sum_title   += stats.len_title   as f64;
        sum_headers += stats.len_headers as f64;
        sum_code    += stats.len_code    as f64;
        sum_body    += stats.len_body    as f64;
    }

    AvgLengths {
        title:   (sum_title   / n) as f32,
        headers: (sum_headers / n) as f32,
        code:    (sum_code    / n) as f32,
        body:    (sum_body    / n) as f32,
    }
}

pub struct BM25FParams {
    pub k1:        f32,
    pub w_title:   f32,
    pub w_headers: f32,
    pub w_code:    f32,
    pub w_body:    f32,
    pub b_title:   f32,
    pub b_headers: f32,
    pub b_code:    f32,
    pub b_body:    f32,
}

impl Default for BM25FParams {
    fn default() -> Self {
        BM25FParams {
            k1:        1.2,
            w_title:   5.0,
            w_headers: 2.5,
            w_code:    1.5,
            w_body:    1.0,
            b_title:   0.3,
            b_headers: 0.5,
            b_code:    0.7,
            b_body:    0.75,
        }
    }
}

// IDF component of BM25F
pub fn bm25_idf(n: f32, df: f32) -> f32 {
    ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
}

// normalize a single field's tf for length — returns 0 if field is absent or corpus has no coverage
pub fn normalize_field_tf(tf: f32, b: f32, dl: f32, avgdl: f32) -> f32 {
    if tf == 0.0 || avgdl == 0.0 { return 0.0; }
    tf / (1.0 - b + b * (dl / avgdl))
}

// score one (term, doc) pair across all fields
pub fn bm25f_score(
    tf_title: f32, tf_headers: f32, tf_code: f32, tf_body: f32,
    df: f32, n: f32,
    doc_stats: &DocStats,
    avg: &AvgLengths,
    params: &BM25FParams,
) -> f32 {
    // no term frequency in any field — skip immediately
    if tf_title == 0.0 && tf_headers == 0.0 && tf_code == 0.0 && tf_body == 0.0 {
        return 0.0;
    }

    let idf = bm25_idf(n, df);

    // normalize each field's tf for length, then apply field weight
    let tilde_tf =
        params.w_title   * normalize_field_tf(tf_title,   params.b_title,   doc_stats.len_title   as f32, avg.title)
      + params.w_headers * normalize_field_tf(tf_headers, params.b_headers, doc_stats.len_headers as f32, avg.headers)
      + params.w_code    * normalize_field_tf(tf_code,    params.b_code,    doc_stats.len_code    as f32, avg.code)
      + params.w_body    * normalize_field_tf(tf_body,    params.b_body,    doc_stats.len_body    as f32, avg.body);

    // BM25F closed form
    idf * tilde_tf * (params.k1 + 1.0) / (tilde_tf + params.k1)
}

//a ranking function which uses BM25F — replaces old tf-idf + cosine normalization + omega boost
pub fn rank_results(
    candidates:  Vec<u32>,
    query_terms: &Vec<String>,
    term_index:  &HashMap<String, TermEntry>,
    doc_stats:   &HashMap<u32, DocStats>,
    avg_lengths: &AvgLengths,
    params:      &BM25FParams,
    total_docs:  f32,
) -> Vec<(u32, f32)> {
    let n_results = candidates.len();

    if VERBOSE {
        println!("--- RANKING (BM25F) ---");
        println!("  Candidate docs: {}", n_results);
        println!("  Query terms: {} ({:?})", query_terms.len(), query_terms);
    }

    // read postings once per query term — not per doc
    let t_postings = Instant::now();
    let mut term_postings: HashMap<String, HashMap<u32, HashMap<Field, Vec<u32>>>> = HashMap::new();
    for term in query_terms {
        if let Some(postings) = read_postings(term, term_index) {
            term_postings.insert(term.clone(), postings);
        }
    }
    if VERBOSE {
        println!("  Postings read for {} terms in {:?}", query_terms.len(), t_postings.elapsed());
    }

    let t_score = Instant::now();
    let mut ranked_docs: Vec<(u32, f32)> = Vec::new();

    for doc_id in candidates {
        let mut doc_score: f32 = 0.0;

        // get this doc's field lengths — if missing, skip (shouldn't happen)
        let stats = match doc_stats.get(&doc_id) {
            Some(s) => s,
            None => continue,
        };

        for term in query_terms {
            let df = match term_index.get(term) {
                Some(entry) => entry.doc_freq as f32,
                None => continue,
            };

            // if intersect_all is correct, every candidate doc has postings for every query term
            let field_map = term_postings
                .get(term)
                .and_then(|p| p.get(&doc_id))
                .expect("intersect_all returned a doc without postings for this term");

            // extract tf per field — 0 if field absent for this term in this doc
            let tf_title   = field_map.get(&Field::Title)  .map(|v| v.len() as f32).unwrap_or(0.0);
            let tf_headers = field_map.get(&Field::Headers).map(|v| v.len() as f32).unwrap_or(0.0);
            let tf_code    = field_map.get(&Field::Code)   .map(|v| v.len() as f32).unwrap_or(0.0);
            let tf_body    = field_map.get(&Field::Body)   .map(|v| v.len() as f32).unwrap_or(0.0);

            doc_score += bm25f_score(
                tf_title, tf_headers, tf_code, tf_body,
                df, total_docs,
                stats,
                avg_lengths,
                params,
            );
        }

        ranked_docs.push((doc_id, doc_score));
    }

    if VERBOSE {
        println!("  Scoring loop ({} docs) in {:?}", n_results, t_score.elapsed());
    }

    // sort descending by score
    let t_sort = Instant::now();
    ranked_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if VERBOSE {
        println!("  Sort time: {:?}", t_sort.elapsed());
        println!("  Top 5 preview:");
        for (i, (doc_id, score)) in ranked_docs.iter().take(5).enumerate() {
            println!("    {}. doc_id={}  score={:.4}", i + 1, doc_id, score);
        }
        println!();
    }

    ranked_docs
}