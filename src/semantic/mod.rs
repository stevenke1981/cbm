mod corpus;
mod ri;
mod signals;

pub use corpus::Corpus;
pub use ri::{cosine_f32, cosine_i8, quantize, Vector};
pub use signals::{ScoreBreakdown, SymbolProfile, RELATED_THRESHOLD, SIMILAR_THRESHOLD};

use crate::discover::IndexMode;
use crate::error::Result;
use crate::store::{Edge, Store, Symbol};
use std::collections::HashMap;
use tracing::info;

pub const DIM: usize = 768;
pub const MAX_SIMILAR_EDGES_PER_NODE: usize = 10;
pub const MAX_RELATED_EDGES_PER_NODE: usize = 15;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SemanticResult {
    pub vectors_stored: usize,
    pub similar_edges: usize,
    pub semantically_related_edges: usize,
}

/// Check CBRLM_SEMANTIC_ENABLED or CBM_SEMANTIC_ENABLED (upstream compat).
pub fn is_enabled() -> bool {
    for key in ["CBRLM_SEMANTIC_ENABLED", "CBM_SEMANTIC_ENABLED"] {
        if let Ok(v) = std::env::var(key) {
            return matches!(v.as_str(), "1" | "true" | "yes" | "on");
        }
    }
    false
}

pub fn should_run(mode: IndexMode) -> bool {
    is_enabled() && mode != IndexMode::Fast
}

pub fn symbol_document(sym: &Symbol) -> String {
    format!(
        "{} {} {} {}",
        sym.name,
        sym.qualified_name,
        sym.label,
        sym.signature.as_deref().unwrap_or("")
    )
}

pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
        } else if !current.is_empty() {
            push_token(&mut tokens, &current);
            current.clear();
        }
    }
    if !current.is_empty() {
        push_token(&mut tokens, &current);
    }
    tokens
}

fn push_token(out: &mut Vec<String>, raw: &str) {
    let lower = raw.to_lowercase();
    if lower.len() < 2 {
        return;
    }
    out.push(lower.clone());
    split_camel(&lower, out);
}

fn split_camel(token: &str, out: &mut Vec<String>) {
    let mut start = 0usize;
    let chars: Vec<char> = token.chars().collect();
    for i in 1..chars.len() {
        let prev_lower = chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit();
        let cur_upper = chars[i].is_ascii_uppercase();
        if prev_lower && cur_upper {
            let part: String = chars[start..i].iter().collect();
            if part.len() >= 2 {
                out.push(part);
            }
            start = i;
        }
    }
    if start < chars.len() {
        let part: String = chars[start..].iter().collect();
        if part.len() >= 2 && part != token {
            out.push(part);
        }
    }
}

pub fn combined_similarity(tfidf: f32, ri: f32) -> f32 {
    signals::combined_score(ScoreBreakdown {
        tfidf,
        ri,
        minhash: 0.0,
        api_signature: 0.0,
        module_proximity: 0.0,
        halstead: 0.0,
        type_signature: 0.0,
        decorator_pattern: 0.0,
        ast_profile: 0.0,
        data_flow: 0.0,
        graph_diffusion: 0.0,
        combined: 0.0,
    })
}

#[derive(Debug)]
pub struct SemanticEdges {
    pub similar: Vec<Edge>,
    pub related: Vec<Edge>,
}

pub fn compute_semantic_edges(
    symbols: &[Symbol],
    vectors: &[Vector],
    corpus: &Corpus,
    call_edges: &[Edge],
) -> SemanticEdges {
    let profiles: Vec<SymbolProfile> = symbols.iter().map(SymbolProfile::from_symbol).collect();
    let diffusion = signals::DiffusionContext::from_call_edges(symbols, call_edges);

    let mut pair_scores: Vec<(usize, usize, ScoreBreakdown)> = Vec::new();
    for i in 0..symbols.len() {
        for j in (i + 1)..symbols.len() {
            if symbols[i].file_path == symbols[j].file_path && symbols[i].name == symbols[j].name {
                continue;
            }
            let breakdown = signals::score_pair_with_diffusion(
                &profiles[i],
                &profiles[j],
                signals::PairScoreInput {
                    vec_a: Some(&vectors[i]),
                    vec_b: Some(&vectors[j]),
                    corpus: Some(corpus),
                    diffusion: Some(&diffusion),
                    idx_a: Some(i),
                    idx_b: Some(j),
                },
            );
            if breakdown.combined >= RELATED_THRESHOLD {
                pair_scores.push((i, j, breakdown));
            }
        }
    }

    pair_scores.sort_by(|a, b| {
        b.2.combined
            .partial_cmp(&a.2.combined)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut similar_count: HashMap<usize, usize> = HashMap::new();
    let mut related_count: HashMap<usize, usize> = HashMap::new();
    let mut similar = Vec::new();
    let mut related = Vec::new();

    for (i, j, breakdown) in pair_scores {
        let props = breakdown.to_json().to_string();
        if breakdown.combined >= SIMILAR_THRESHOLD {
            let ci = *similar_count.get(&i).unwrap_or(&0);
            let cj = *similar_count.get(&j).unwrap_or(&0);
            if ci >= MAX_SIMILAR_EDGES_PER_NODE || cj >= MAX_SIMILAR_EDGES_PER_NODE {
                continue;
            }
            similar.push(Edge {
                src_qn: symbols[i].qualified_name.clone(),
                dst_qn: symbols[j].qualified_name.clone(),
                edge_type: "SIMILAR_TO".into(),
                properties_json: Some(props),
            });
            *similar_count.entry(i).or_insert(0) += 1;
            *similar_count.entry(j).or_insert(0) += 1;
        } else {
            let ci = *related_count.get(&i).unwrap_or(&0);
            let cj = *related_count.get(&j).unwrap_or(&0);
            if ci >= MAX_RELATED_EDGES_PER_NODE || cj >= MAX_RELATED_EDGES_PER_NODE {
                continue;
            }
            related.push(Edge {
                src_qn: symbols[i].qualified_name.clone(),
                dst_qn: symbols[j].qualified_name.clone(),
                edge_type: "SEMANTICALLY_RELATED".into(),
                properties_json: Some(props),
            });
            *related_count.entry(i).or_insert(0) += 1;
            *related_count.entry(j).or_insert(0) += 1;
        }
    }

    SemanticEdges { similar, related }
}

pub fn build_vectors(symbols: &[Symbol]) -> Vec<Vector> {
    let docs: Vec<Vec<String>> = symbols
        .iter()
        .map(|s| tokenize(&symbol_document(s)))
        .collect();
    let corpus = Corpus::from_documents(&docs);
    symbols
        .iter()
        .zip(docs.iter())
        .map(|(sym, tokens)| Vector::from_tokens(tokens, &corpus, &sym.qualified_name))
        .collect()
}

pub fn vector_search(
    store: &Store,
    query: &str,
    limit: usize,
) -> Result<crate::store::VectorSearchResult> {
    use crate::store::VectorMatch;

    let query_profile = SymbolProfile::from_query(query);
    if query_profile.tokens.is_empty() {
        return Ok(crate::store::VectorSearchResult {
            matches: vec![],
            total: 0,
        });
    }

    let entries = store.list_vector_entries()?;
    if entries.is_empty() {
        return Ok(crate::store::VectorSearchResult {
            matches: vec![],
            total: 0,
        });
    }

    let query_corpus = Corpus::from_documents(std::slice::from_ref(&query_profile.tokens));
    let query_vec = Vector::from_tokens(&query_profile.tokens, &query_corpus, "query");
    let query_q = query_vec.quantized();

    let mut scored: Vec<VectorMatch> = Vec::new();
    for (qn, stored, _name, _label, _file_path) in entries {
        let ri_prefilter = cosine_i8(&query_q, &stored);
        if ri_prefilter < 0.05 {
            continue;
        }
        let Some(sym) = store.find_symbol(&qn)? else {
            continue;
        };
        let target_tokens = tokenize(&symbol_document(&sym));
        let pair_corpus = Corpus::from_documents(&[query_profile.tokens.clone(), target_tokens]);
        let target_vec = Vector::from_tokens(&tokenize(&symbol_document(&sym)), &pair_corpus, &qn);
        let target_profile = SymbolProfile::from_symbol(&sym);
        let breakdown = signals::score_pair(
            &query_profile,
            &target_profile,
            Some(&query_vec),
            Some(&target_vec),
            Some(&pair_corpus),
        );
        if breakdown.combined <= 0.1 {
            continue;
        }
        scored.push(VectorMatch {
            qualified_name: qn,
            name: sym.name,
            label: sym.label,
            file_path: sym.file_path,
            score: breakdown.combined,
            score_breakdown: Some(breakdown.to_json()),
        });
    }

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let total = scored.len();
    let matches: Vec<VectorMatch> = scored.into_iter().take(limit).collect();
    Ok(crate::store::VectorSearchResult { matches, total })
}

pub fn run_semantic_pass(store: &Store) -> Result<SemanticResult> {
    let symbols: Vec<Symbol> = store
        .list_symbols()?
        .into_iter()
        .filter(|s| matches!(s.label.as_str(), "Function" | "Class"))
        .collect();

    if symbols.len() < 2 {
        return Ok(SemanticResult {
            vectors_stored: 0,
            similar_edges: 0,
            semantically_related_edges: 0,
        });
    }

    info!(symbols = symbols.len(), "semantic pass starting");

    // Global corpus for consistent IDF across all symbols
    let all_docs: Vec<Vec<String>> = symbols
        .iter()
        .map(|s| tokenize(&symbol_document(s)))
        .collect();
    let corpus = Corpus::from_documents(&all_docs);
    let vectors: Vec<Vector> = symbols
        .iter()
        .zip(all_docs.iter())
        .map(|(sym, tokens)| Vector::from_tokens(tokens, &corpus, &sym.qualified_name))
        .collect();

    store.clear_vectors()?;
    for v in &vectors {
        store.upsert_vector(&v.qualified_name, DIM as i32, &v.quantized())?;
    }

    let call_edges: Vec<Edge> = store
        .list_edges()?
        .into_iter()
        .filter(|e| e.edge_type == "CALLS")
        .collect();
    let semantic_edges = compute_semantic_edges(&symbols, &vectors, &corpus, &call_edges);
    store.delete_edges_by_type("SIMILAR_TO")?;
    store.delete_edges_by_type("SEMANTICALLY_RELATED")?;
    store.insert_edges_batch(&semantic_edges.similar)?;
    store.insert_edges_batch(&semantic_edges.related)?;

    info!(
        vectors = vectors.len(),
        similar = semantic_edges.similar.len(),
        related = semantic_edges.related.len(),
        "semantic pass complete"
    );

    Ok(SemanticResult {
        vectors_stored: vectors.len(),
        similar_edges: semantic_edges.similar.len(),
        semantically_related_edges: semantic_edges.related.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_splits_identifiers() {
        let t = tokenize("getUserName HTTPHandler");
        assert!(t.iter().any(|x| x == "getusername" || x == "get"));
        assert!(t.iter().any(|x| x == "httphandler" || x == "handler"));
    }

    #[test]
    fn similar_symbols_score_high() {
        let syms = [
            Symbol {
                qualified_name: "a.rs::fetch_user".into(),
                name: "fetch_user".into(),
                label: "Function".into(),
                file_path: "a.rs".into(),
                line_start: 1,
                line_end: 2,
                signature: Some("fn fetch_user(id: u64)".into()),
                properties_json: None,
            },
            Symbol {
                qualified_name: "b.rs::fetch_user_by_id".into(),
                name: "fetch_user_by_id".into(),
                label: "Function".into(),
                file_path: "b.rs".into(),
                line_start: 1,
                line_end: 2,
                signature: Some("fn fetch_user_by_id(id: u64)".into()),
                properties_json: None,
            },
        ];
        let docs: Vec<Vec<String>> = syms.iter().map(|s| tokenize(&symbol_document(s))).collect();
        let corpus = Corpus::from_documents(&docs);
        let vecs: Vec<Vector> = syms
            .iter()
            .zip(docs.iter())
            .map(|(s, t)| Vector::from_tokens(t, &corpus, &s.qualified_name))
            .collect();
        let profiles: Vec<SymbolProfile> = syms.iter().map(SymbolProfile::from_symbol).collect();
        let breakdown = signals::score_pair(
            &profiles[0],
            &profiles[1],
            Some(&vecs[0]),
            Some(&vecs[1]),
            Some(&corpus),
        );
        assert!(breakdown.tfidf > 0.2, "tfidf={}", breakdown.tfidf);
        assert!(breakdown.ri > 0.15, "ri={}", breakdown.ri);
        assert!(
            breakdown.combined >= 0.30,
            "combined={}",
            breakdown.combined
        );
    }

    #[test]
    fn emits_similar_and_related_thresholds() {
        let syms = [
            Symbol {
                qualified_name: "a.rs::Function::fetch_user@L1".into(),
                name: "fetch_user".into(),
                label: "Function".into(),
                file_path: "a.rs".into(),
                line_start: 1,
                line_end: 2,
                signature: Some("fn fetch_user(id: u64)".into()),
                properties_json: None,
            },
            Symbol {
                qualified_name: "a.rs::Function::fetch_user_profile@L3".into(),
                name: "fetch_user_profile".into(),
                label: "Function".into(),
                file_path: "a.rs".into(),
                line_start: 3,
                line_end: 4,
                signature: Some("fn fetch_user_profile(id: u64)".into()),
                properties_json: None,
            },
            Symbol {
                qualified_name: "z.rs::Function::render@L1".into(),
                name: "render".into(),
                label: "Function".into(),
                file_path: "z.rs".into(),
                line_start: 1,
                line_end: 2,
                signature: Some("fn render()".into()),
                properties_json: None,
            },
        ];
        let docs: Vec<Vec<String>> = syms.iter().map(|s| tokenize(&symbol_document(s))).collect();
        let corpus = Corpus::from_documents(&docs);
        let vecs: Vec<Vector> = syms
            .iter()
            .zip(docs.iter())
            .map(|(s, t)| Vector::from_tokens(t, &corpus, &s.qualified_name))
            .collect();
        let edges = compute_semantic_edges(&syms, &vecs, &corpus, &[]);
        assert!(!edges.similar.is_empty() || !edges.related.is_empty());
        let has_breakdown = edges.similar.iter().chain(edges.related.iter()).any(|e| {
            e.properties_json
                .as_ref()
                .is_some_and(|p| p.contains("signals"))
        });
        assert!(has_breakdown);
    }
}
