use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Corpus {
    idf: HashMap<String, f32>,
}

impl Corpus {
    pub fn from_documents(docs: &[Vec<String>]) -> Self {
        let doc_count = docs.len().max(1);
        let mut df: HashMap<String, usize> = HashMap::new();
        for doc in docs {
            let mut seen = std::collections::HashSet::new();
            for token in doc {
                if seen.insert(token.clone()) {
                    *df.entry(token.clone()).or_insert(0) += 1;
                }
            }
        }
        let idf = df
            .into_iter()
            .map(|(term, freq)| {
                let weight = ((doc_count as f32 + 1.0) / (freq as f32 + 1.0)).ln() + 1.0;
                (term, weight)
            })
            .collect();
        Self { idf }
    }

    pub fn idf(&self, token: &str) -> f32 {
        *self.idf.get(token).unwrap_or(&1.0)
    }

    pub fn tfidf_vector(&self, tokens: &[String]) -> HashMap<String, f32> {
        let mut tf: HashMap<String, usize> = HashMap::new();
        for t in tokens {
            *tf.entry(t.clone()).or_insert(0) += 1;
        }
        let len = tokens.len().max(1) as f32;
        tf.into_iter()
            .map(|(term, count)| {
                let tf_w = count as f32 / len;
                (term.clone(), tf_w * self.idf(&term))
            })
            .collect()
    }

    pub fn cosine_sparse(a: &HashMap<String, f32>, b: &HashMap<String, f32>) -> f32 {
        let mut dot = 0.0f32;
        let mut na = 0.0f32;
        let mut nb = 0.0f32;
        for (k, v) in a {
            na += v * v;
            if let Some(bv) = b.get(k) {
                dot += v * bv;
            }
        }
        for v in b.values() {
            nb += v * v;
        }
        if na == 0.0 || nb == 0.0 {
            return 0.0;
        }
        dot / (na.sqrt() * nb.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_docs_cosine_one() {
        let doc = vec!["hello".into(), "world".into()];
        let corpus = Corpus::from_documents(std::slice::from_ref(&doc));
        let v1 = corpus.tfidf_vector(&doc);
        let v2 = corpus.tfidf_vector(&doc);
        assert!((Corpus::cosine_sparse(&v1, &v2) - 1.0).abs() < 0.01);
    }
}
