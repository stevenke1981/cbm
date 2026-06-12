use base64::{engine::general_purpose::STANDARD, Engine};
use lz4_flex::{compress_prepend_size, decompress_size_prepended};

const PREFIX: &str = "CBRLM_LZ4:";
const MIN_COMPRESS_BYTES: usize = 2048;

pub fn maybe_compress(content: &str) -> String {
    if content.len() < MIN_COMPRESS_BYTES {
        return content.to_string();
    }
    let compressed = compress_prepend_size(content.as_bytes());
    let encoded = STANDARD.encode(compressed);
    let out = format!("{PREFIX}{encoded}");
    if out.len() < content.len() {
        out
    } else {
        content.to_string()
    }
}

pub fn maybe_decompress(stored: &str) -> String {
    let Some(encoded) = stored.strip_prefix(PREFIX) else {
        return stored.to_string();
    };
    let Ok(bytes) = STANDARD.decode(encoded) else {
        return stored.to_string();
    };
    let Ok(raw) = decompress_size_prepended(&bytes) else {
        return stored.to_string();
    };
    String::from_utf8_lossy(&raw).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_large_content() {
        let input = "fn main() {\n".repeat(400);
        let compressed = maybe_compress(&input);
        assert!(compressed.starts_with(PREFIX));
        assert!(compressed.len() < input.len());
        assert_eq!(maybe_decompress(&compressed), input);
    }

    #[test]
    fn skips_small_content() {
        let input = "short";
        assert_eq!(maybe_compress(input), input);
    }
}
