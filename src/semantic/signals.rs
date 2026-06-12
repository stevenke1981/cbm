//! Modular semantic similarity signals (reference spec slice).

use super::corpus::Corpus;
use super::ri::Vector;
use super::tokenize;
use crate::store::Symbol;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub const MINHASH_BUCKETS: usize = 64;
pub const SHINGLE_SIZE: usize = 3;

pub const SIMILAR_THRESHOLD: f32 = 0.58;
pub const RELATED_THRESHOLD: f32 = 0.38;

#[derive(Debug, Clone, Serialize)]
pub struct ScoreBreakdown {
    pub tfidf: f32,
    pub ri: f32,
    pub minhash: f32,
    pub api_signature: f32,
    pub module_proximity: f32,
    pub halstead: f32,
    pub type_signature: f32,
    pub decorator_pattern: f32,
    pub ast_profile: f32,
    pub data_flow: f32,
    pub graph_diffusion: f32,
    pub combined: f32,
}

impl ScoreBreakdown {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "score": self.combined,
            "signals": {
                "tfidf": self.tfidf,
                "ri": self.ri,
                "minhash": self.minhash,
                "api_signature": self.api_signature,
                "module_proximity": self.module_proximity,
                "halstead": self.halstead,
                "type_signature": self.type_signature,
                "decorator_pattern": self.decorator_pattern,
                "ast_profile": self.ast_profile,
                "data_flow": self.data_flow,
                "graph_diffusion": self.graph_diffusion,
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct HalsteadMetrics {
    pub operators: usize,
    pub operands: usize,
}

#[derive(Debug, Clone)]
pub struct SymbolProfile {
    pub tokens: Vec<String>,
    pub minhash: [u64; MINHASH_BUCKETS],
    pub api_tokens: Vec<String>,
    pub type_tokens: Vec<String>,
    pub decorator_tokens: Vec<String>,
    pub ast_histogram: [f32; 4],
    pub identifier_tokens: Vec<String>,
    pub halstead: HalsteadMetrics,
    pub file_path: String,
    pub qualified_name: String,
}

impl SymbolProfile {
    pub fn from_symbol(sym: &Symbol) -> Self {
        let doc = super::symbol_document(sym);
        let sig = sym.signature.as_deref().unwrap_or("");
        let tokens = tokenize(&doc);
        let api_tokens = api_signature_tokens(sig);
        Self {
            minhash: minhash_sketch(&shingles(&tokens, SHINGLE_SIZE)),
            halstead: halstead_metrics(sig),
            type_tokens: type_signature_tokens(sig),
            decorator_tokens: decorator_tokens(sig),
            ast_histogram: ast_histogram(sig, &sym.name),
            identifier_tokens: identifier_tokens(sig, &sym.name),
            tokens,
            api_tokens,
            file_path: sym.file_path.clone(),
            qualified_name: sym.qualified_name.clone(),
        }
    }

    pub fn from_query(query: &str) -> Self {
        let tokens = tokenize(query);
        Self {
            minhash: minhash_sketch(&shingles(&tokens, SHINGLE_SIZE)),
            halstead: halstead_metrics(query),
            type_tokens: type_signature_tokens(query),
            decorator_tokens: decorator_tokens(query),
            ast_histogram: ast_histogram(query, "query"),
            identifier_tokens: identifier_tokens(query, "query"),
            tokens: tokens.clone(),
            api_tokens: api_signature_tokens(query),
            file_path: String::new(),
            qualified_name: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiffusionContext {
    pub neighbor_sets: Vec<HashSet<String>>,
}

pub struct PairScoreInput<'a> {
    pub vec_a: Option<&'a Vector>,
    pub vec_b: Option<&'a Vector>,
    pub corpus: Option<&'a Corpus>,
    pub diffusion: Option<&'a DiffusionContext>,
    pub idx_a: Option<usize>,
    pub idx_b: Option<usize>,
}

impl DiffusionContext {
    pub fn from_call_edges(symbols: &[Symbol], edges: &[crate::store::Edge]) -> Self {
        let index: HashMap<String, usize> = symbols
            .iter()
            .enumerate()
            .map(|(i, s)| (s.qualified_name.clone(), i))
            .collect();
        let mut neighbor_sets = vec![HashSet::new(); symbols.len()];
        for edge in edges {
            if edge.edge_type != "CALLS" {
                continue;
            }
            if let (Some(&a), Some(&b)) = (index.get(&edge.src_qn), index.get(&edge.dst_qn)) {
                neighbor_sets[a].insert(edge.dst_qn.clone());
                neighbor_sets[b].insert(edge.src_qn.clone());
            }
        }
        Self { neighbor_sets }
    }
}

pub fn score_pair(
    a: &SymbolProfile,
    b: &SymbolProfile,
    vec_a: Option<&Vector>,
    vec_b: Option<&Vector>,
    corpus: Option<&Corpus>,
) -> ScoreBreakdown {
    score_pair_with_diffusion(
        a,
        b,
        PairScoreInput {
            vec_a,
            vec_b,
            corpus,
            diffusion: None,
            idx_a: None,
            idx_b: None,
        },
    )
}

pub fn score_pair_with_diffusion(
    a: &SymbolProfile,
    b: &SymbolProfile,
    input: PairScoreInput<'_>,
) -> ScoreBreakdown {
    let PairScoreInput {
        vec_a,
        vec_b,
        corpus,
        diffusion,
        idx_a,
        idx_b,
    } = input;
    let tfidf = match (vec_a, vec_b) {
        (Some(va), Some(vb)) => va.tfidf_similarity(vb),
        _ => corpus
            .map(|c| Corpus::cosine_sparse(&c.tfidf_vector(&a.tokens), &c.tfidf_vector(&b.tokens)))
            .unwrap_or(0.0),
    };
    let ri = match (vec_a, vec_b) {
        (Some(va), Some(vb)) => super::ri::cosine_f32(&va.ri, &vb.ri),
        _ => 0.0,
    };
    let minhash = minhash_similarity(&a.minhash, &b.minhash);
    let api_signature = api_similarity(&a.api_tokens, &b.api_tokens);
    let module_proximity = module_proximity(&a.file_path, &b.file_path);
    let halstead = halstead_similarity(&a.halstead, &b.halstead);
    let type_signature = token_jaccard(&a.type_tokens, &b.type_tokens);
    let decorator_pattern = token_jaccard(&a.decorator_tokens, &b.decorator_tokens);
    let ast_profile = histogram_cosine(&a.ast_histogram, &b.ast_histogram);
    let data_flow = token_jaccard(&a.identifier_tokens, &b.identifier_tokens);
    let graph_diffusion = match (diffusion, idx_a, idx_b) {
        (Some(ctx), Some(i), Some(j)) => {
            neighbor_jaccard(&ctx.neighbor_sets[i], &ctx.neighbor_sets[j])
        }
        _ => 0.0,
    };
    let combined = combined_score(ScoreBreakdown {
        tfidf,
        ri,
        minhash,
        api_signature,
        module_proximity,
        halstead,
        type_signature,
        decorator_pattern,
        ast_profile,
        data_flow,
        graph_diffusion,
        combined: 0.0,
    });
    ScoreBreakdown {
        tfidf,
        ri,
        minhash,
        api_signature,
        module_proximity,
        halstead,
        type_signature,
        decorator_pattern,
        ast_profile,
        data_flow,
        graph_diffusion,
        combined,
    }
}

pub fn combined_score(parts: ScoreBreakdown) -> f32 {
    0.18 * parts.tfidf
        + 0.15 * parts.ri
        + 0.10 * parts.minhash
        + 0.08 * parts.api_signature
        + 0.07 * parts.module_proximity
        + 0.06 * parts.halstead
        + 0.10 * parts.type_signature
        + 0.06 * parts.decorator_pattern
        + 0.08 * parts.ast_profile
        + 0.07 * parts.data_flow
        + 0.05 * parts.graph_diffusion
}

fn shingles(tokens: &[String], k: usize) -> Vec<String> {
    if tokens.is_empty() {
        return Vec::new();
    }
    if tokens.len() < k {
        return vec![tokens.join("_")];
    }
    tokens.windows(k).map(|w| w.join("_")).collect()
}

fn minhash_sketch(shingles: &[String]) -> [u64; MINHASH_BUCKETS] {
    let mut sketch = [u64::MAX; MINHASH_BUCKETS];
    for shingle in shingles {
        for (i, slot) in sketch.iter_mut().enumerate() {
            let h = hash64(shingle, i as u64);
            if h < *slot {
                *slot = h;
            }
        }
    }
    if shingles.is_empty() {
        return [0; MINHASH_BUCKETS];
    }
    sketch
}

fn minhash_similarity(a: &[u64; MINHASH_BUCKETS], b: &[u64; MINHASH_BUCKETS]) -> f32 {
    let matches = a.iter().zip(b.iter()).filter(|(x, y)| x == y).count();
    matches as f32 / MINHASH_BUCKETS as f32
}

fn hash64(s: &str, seed: u64) -> u64 {
    let mut h = 14695981039346656037u64 ^ seed;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

fn api_signature_tokens(sig: &str) -> Vec<String> {
    if sig.is_empty() {
        return Vec::new();
    }
    let mut tokens = tokenize(sig);
    tokens.retain(|t| {
        !matches!(
            t.as_str(),
            "fn" | "pub" | "async" | "mut" | "self" | "impl" | "def" | "class" | "void" | "return"
        )
    });
    tokens
}

fn api_similarity(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut tf: HashMap<&str, usize> = HashMap::new();
    for t in a {
        *tf.entry(t.as_str()).or_insert(0) += 1;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for t in b {
        let w = *tf.get(t.as_str()).unwrap_or(&0) as f32;
        dot += w;
        nb += 1.0;
    }
    for c in tf.values() {
        na += *c as f32 * *c as f32;
    }
    nb = nb.sqrt();
    na = na.sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    (dot / na.max(nb)).clamp(0.0, 1.0)
}

pub fn module_proximity(path_a: &str, path_b: &str) -> f32 {
    if path_a.is_empty() || path_b.is_empty() {
        return 0.0;
    }
    if path_a == path_b {
        return 1.0;
    }
    let a: Vec<&str> = path_a.split('/').collect();
    let b: Vec<&str> = path_b.split('/').collect();
    let common = a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count();
    if common == 0 {
        return 0.0;
    }
    let max_depth = a.len().max(b.len()) as f32;
    (common as f32 / max_depth).clamp(0.0, 1.0)
}

fn halstead_metrics(sig: &str) -> HalsteadMetrics {
    let operators = sig
        .chars()
        .filter(|c| {
            matches!(
                c,
                '(' | ')' | '{' | '}' | '[' | ']' | ';' | ',' | ':' | '<' | '>'
            )
        })
        .count();
    let operands = tokenize(sig).len();
    HalsteadMetrics {
        operators,
        operands,
    }
}

fn type_signature_tokens(sig: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in sig.split(|c: char| !c.is_alphanumeric() && c != '_') {
        let p = part.trim();
        if p.len() >= 2
            && p.chars()
                .next()
                .is_some_and(|c| c.is_uppercase() || c == '&')
            && !matches!(p, "Self" | "String" | "Vec" | "Option" | "Result")
        {
            out.push(p.to_ascii_lowercase());
        }
    }
    out
}

fn decorator_tokens(sig: &str) -> Vec<String> {
    sig.lines()
        .filter_map(|line| {
            let t = line.trim();
            if t.starts_with('#') || t.starts_with('@') {
                Some(tokenize(t))
            } else {
                None
            }
        })
        .flatten()
        .collect()
}

fn ast_histogram(sig: &str, name: &str) -> [f32; 4] {
    let kw = [
        "fn", "def", "class", "impl", "async", "pub", "return", "if", "for",
    ];
    let mut counts = [0f32; 4];
    for token in tokenize(sig).iter().chain(tokenize(name).iter()) {
        if kw.contains(&token.as_str()) {
            counts[0] += 1.0;
        } else if token.chars().all(|c| c.is_ascii_digit()) {
            counts[3] += 1.0;
        } else if token.chars().any(|c| !c.is_alphanumeric() && c != '_') {
            counts[2] += 1.0;
        } else {
            counts[1] += 1.0;
        }
    }
    counts
}

fn identifier_tokens(sig: &str, name: &str) -> Vec<String> {
    let mut tokens = tokenize(sig);
    tokens.extend(tokenize(name));
    tokens
        .into_iter()
        .filter(|t| t.len() >= 3 && t.chars().all(|c| c.is_alphanumeric() || c == '_'))
        .collect()
}

fn token_jaccard(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let sa: HashSet<&str> = a.iter().map(|s| s.as_str()).collect();
    let sb: HashSet<&str> = b.iter().map(|s| s.as_str()).collect();
    let inter = sa.intersection(&sb).count();
    let union = sa.union(&sb).count().max(1);
    inter as f32 / union as f32
}

fn histogram_cosine(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..4 {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    (dot / (na.sqrt() * nb.sqrt())).clamp(0.0, 1.0)
}

fn neighbor_jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count();
    let union = a.union(b).count().max(1);
    inter as f32 / union as f32
}

fn halstead_similarity(a: &HalsteadMetrics, b: &HalsteadMetrics) -> f32 {
    let va = (a.operators + a.operands) as f32;
    let vb = (b.operators + b.operands) as f32;
    if va == 0.0 && vb == 0.0 {
        return 0.0;
    }
    let denom = va.max(vb).max(1.0);
    (1.0 - (va - vb).abs() / denom).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(qn: &str, name: &str, file: &str, sig: &str) -> Symbol {
        Symbol {
            qualified_name: qn.into(),
            name: name.into(),
            label: "Function".into(),
            file_path: file.into(),
            line_start: 1,
            line_end: 2,
            signature: Some(sig.into()),
            properties_json: None,
        }
    }

    #[test]
    fn same_file_module_proximity_is_high() {
        assert!(module_proximity("src/a.rs", "src/b.rs") >= 0.5);
        assert_eq!(module_proximity("src/a.rs", "src/a.rs"), 1.0);
    }

    #[test]
    fn similar_api_signatures_score_high() {
        let a = SymbolProfile::from_symbol(&sym(
            "a.rs::f@L1",
            "fetch_user",
            "a.rs",
            "fn fetch_user(id: u64) -> User",
        ));
        let b = SymbolProfile::from_symbol(&sym(
            "b.rs::f@L1",
            "fetch_user_by_id",
            "b.rs",
            "fn fetch_user_by_id(id: u64) -> User",
        ));
        let score = score_pair(&a, &b, None, None, None);
        assert!(score.api_signature > 0.3, "api={}", score.api_signature);
        assert!(score.combined > 0.2, "combined={}", score.combined);
    }

    #[test]
    fn unrelated_symbols_score_low() {
        let a = SymbolProfile::from_symbol(&sym("a.rs::f@L1", "render", "ui.rs", "fn render()"));
        let b = SymbolProfile::from_symbol(&sym(
            "b.rs::f@L1",
            "hash_password",
            "auth.rs",
            "fn hash_password(pw: &str)",
        ));
        let score = score_pair(&a, &b, None, None, None);
        assert!(score.combined < SIMILAR_THRESHOLD);
    }
}
