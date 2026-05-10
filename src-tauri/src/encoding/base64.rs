use base64::{engine::general_purpose, Engine as _};

/// Encodes bytes to base64 string using standard encoding
pub fn encode_base64(data: &[u8]) -> String {
    general_purpose::STANDARD.encode(data)
}

/// Splits data into fixed-size chunks
pub fn chunk_data(data: &str, chunk_size: usize) -> Vec<String> {
    data.as_bytes()
        .chunks(chunk_size)
        .map(|chunk| String::from_utf8_lossy(chunk).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_base64_sample_data() {
        let data = b"Hello, World!";
        let encoded = encode_base64(data);
        assert_eq!(encoded, "SGVsbG8sIFdvcmxkIQ==");
    }

    #[test]
    fn test_encode_base64_empty() {
        let data = b"";
        let encoded = encode_base64(data);
        assert_eq!(encoded, "");
    }

    #[test]
    fn test_chunk_data_single_chunk() {
        let data = "Hello";
        let chunks = chunk_data(data, 100);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Hello");
    }

    #[test]
    fn test_chunk_data_multiple_chunks() {
        let data = "0123456789";
        let chunks = chunk_data(data, 5);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "01234");
        assert_eq!(chunks[1], "56789");
    }

    #[test]
    fn test_chunk_data_exact_boundary() {
        let data = "0123456789";
        let chunks = chunk_data(data, 10);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "0123456789");
    }

    #[test]
    fn test_chunk_data_with_remainder() {
        let data = "0123456789ABC";
        let chunks = chunk_data(data, 5);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0], "01234");
        assert_eq!(chunks[1], "56789");
        assert_eq!(chunks[2], "ABC");
    }
}
