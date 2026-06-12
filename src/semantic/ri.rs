use super::corpus::Corpus;
use super::DIM;

#[derive(Debug, Clone)]
pub struct Vector {
    pub qualified_name: String,
    pub ri: Vec<f32>,
    tfidf: std::collections::HashMap<String, f32>,
}

impl Vector {
    pub fn from_tokens(tokens: &[String], corpus: &Corpus, qn: &str) -> Self {
        let tfidf = corpus.tfidf_vector(tokens);
        let mut ri = vec![0.0f32; DIM];
        for token in tokens {
            let weight = corpus.idf(token);
            let h1 = hash_token(token, 0) % DIM;
            let h2 = hash_token(token, 1) % DIM;
            let sign = if hash_token(token, 2).is_multiple_of(2) {
                1.0
            } else {
                -1.0
            };
            ri[h1] += sign * weight;
            ri[h2] += sign * 0.5 * weight;
        }
        l2_normalize(&mut ri);
        Self {
            qualified_name: qn.to_string(),
            ri,
            tfidf,
        }
    }

    pub fn tfidf_similarity(&self, other: &Self) -> f32 {
        Corpus::cosine_sparse(&self.tfidf, &other.tfidf)
    }

    pub fn quantized(&self) -> Vec<i8> {
        quantize(&self.ri)
    }
}

fn hash_token(token: &str, salt: u64) -> usize {
    let mut h = 14695981039346656037u64 ^ salt;
    for b in token.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h as usize
}

pub fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-8 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

pub fn quantize(v: &[f32]) -> Vec<i8> {
    v.iter()
        .map(|&x| (x.clamp(-1.0, 1.0) * 127.0).round() as i8)
        .collect()
}

pub fn cosine_f32(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len().min(b.len()) {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

pub fn cosine_i8(a: &[i8], b: &[i8]) -> f32 {
    let mut dot = 0i32;
    let mut na = 0i32;
    let mut nb = 0i32;
    for i in 0..a.len().min(b.len()) {
        let av = a[i] as i32;
        let bv = b[i] as i32;
        dot += av * bv;
        na += av * av;
        nb += bv * bv;
    }
    if na == 0 || nb == 0 {
        return 0.0;
    }
    dot as f32 / ((na as f32).sqrt() * (nb as f32).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantized_cosine_bounded() {
        let a = quantize(&[1.0, 0.0, 0.5]);
        let b = quantize(&[1.0, 0.0, 0.5]);
        assert!((cosine_i8(&a, &b) - 1.0).abs() < 0.05);
    }
}
