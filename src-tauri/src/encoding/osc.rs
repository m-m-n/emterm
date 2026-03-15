use uuid::Uuid;

/// Sanitize a string for safe embedding in OSC parameter values.
/// Removes semicolons (OSC field delimiter) and control characters.
fn sanitize_osc_value(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c != ';' && !c.is_control())
        .collect()
}

/// Generates OSC 777 sequences for Markdown content
///
/// # Format
/// ```text
/// ESC ] 777 ; emterm ; markdown ; begin ; id={uuid} ; format=gfm ; render=fullscreen ; version=1.0 ESC \
/// ESC ] 777 ; emterm ; markdown ; chunk ; id={uuid} ; seq=N ; data={base64} ESC \
/// ESC ] 777 ; emterm ; markdown ; end ; id={uuid} ESC \
/// ```
pub fn generate_markdown_osc(session_id: &Uuid, chunks: Vec<String>) -> String {
    let total_data: usize = chunks.iter().map(|c| c.len()).sum();
    let header_overhead = 100;
    let estimated = total_data + header_overhead * (chunks.len() + 2);
    let mut output = String::with_capacity(estimated);
    let id = session_id.to_string();

    // Begin sequence
    output.push_str(&format!(
        "\x1b]777;emterm;markdown;begin;id={};format=gfm;render=fullscreen;version=1.0\x1b\\",
        id
    ));

    // Chunk sequences
    for (seq, data) in chunks.iter().enumerate() {
        output.push_str(&format!(
            "\x1b]777;emterm;markdown;chunk;id={};seq={};data={}\x1b\\",
            id, seq, data
        ));
    }

    // End sequence
    output.push_str(&format!("\x1b]777;emterm;markdown;end;id={}\x1b\\", id));

    output
}

/// Generate a single download begin OSC sequence.
pub fn generate_download_osc_begin(session_id: &Uuid, filename: &str, file_size: u64) -> String {
    let safe_filename = sanitize_osc_value(filename);
    format!(
        "\x1b]777;emterm;download;begin;id={};name={};size={};version=1.0\x1b\\",
        session_id, safe_filename, file_size
    )
}

/// Generate a single download chunk OSC sequence.
pub fn generate_download_osc_chunk(session_id: &Uuid, seq: usize, data: &str) -> String {
    format!(
        "\x1b]777;emterm;download;chunk;id={};seq={};data={}\x1b\\",
        session_id, seq, data
    )
}

/// Generate a single download end OSC sequence.
pub fn generate_download_osc_end(session_id: &Uuid) -> String {
    format!("\x1b]777;emterm;download;end;id={}\x1b\\", session_id)
}

/// Generates OSC 777 sequences for JSON content
///
/// # Format
/// ```text
/// ESC ] 777 ; emterm ; json ; begin ; id={uuid} ; version=1.0 ESC \
/// ESC ] 777 ; emterm ; json ; chunk ; id={uuid} ; seq=N ; data={base64} ESC \
/// ESC ] 777 ; emterm ; json ; end ; id={uuid} ESC \
/// ```
pub fn generate_json_osc(session_id: &Uuid, chunks: Vec<String>) -> String {
    let total_data: usize = chunks.iter().map(|c| c.len()).sum();
    let header_overhead = 100;
    let estimated = total_data + header_overhead * (chunks.len() + 2);
    let mut output = String::with_capacity(estimated);
    let id = session_id.to_string();

    // Begin sequence
    output.push_str(&format!(
        "\x1b]777;emterm;json;begin;id={};version=1.0\x1b\\",
        id
    ));

    // Chunk sequences
    for (seq, data) in chunks.iter().enumerate() {
        output.push_str(&format!(
            "\x1b]777;emterm;json;chunk;id={};seq={};data={}\x1b\\",
            id, seq, data
        ));
    }

    // End sequence
    output.push_str(&format!("\x1b]777;emterm;json;end;id={}\x1b\\", id));

    output
}

/// Generates OSC 777 sequences for YAML content
///
/// # Format
/// ```text
/// ESC ] 777 ; emterm ; yaml ; begin ; id={uuid} ; version=1.0 ESC \
/// ESC ] 777 ; emterm ; yaml ; chunk ; id={uuid} ; seq=N ; data={base64} ESC \
/// ESC ] 777 ; emterm ; yaml ; end ; id={uuid} ESC \
/// ```
pub fn generate_yaml_osc(session_id: &Uuid, chunks: Vec<String>) -> String {
    let total_data: usize = chunks.iter().map(|c| c.len()).sum();
    let header_overhead = 100;
    let estimated = total_data + header_overhead * (chunks.len() + 2);
    let mut output = String::with_capacity(estimated);
    let id = session_id.to_string();

    // Begin sequence
    output.push_str(&format!(
        "\x1b]777;emterm;yaml;begin;id={};version=1.0\x1b\\",
        id
    ));

    // Chunk sequences
    for (seq, data) in chunks.iter().enumerate() {
        output.push_str(&format!(
            "\x1b]777;emterm;yaml;chunk;id={};seq={};data={}\x1b\\",
            id, seq, data
        ));
    }

    // End sequence
    output.push_str(&format!("\x1b]777;emterm;yaml;end;id={}\x1b\\", id));

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper: generate combined download OSC sequences (begin + chunks + end).
    fn generate_download_osc(
        session_id: &Uuid,
        filename: &str,
        file_size: u64,
        chunks: Vec<String>,
    ) -> String {
        let mut output = generate_download_osc_begin(session_id, filename, file_size);
        for (seq, data) in chunks.iter().enumerate() {
            output.push_str(&generate_download_osc_chunk(session_id, seq, data));
        }
        output.push_str(&generate_download_osc_end(session_id));
        output
    }

    #[test]
    fn test_generate_markdown_osc_single_chunk() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["SGVsbG8=".to_string()];

        let result = generate_markdown_osc(&session_id, chunks);

        // Verify structure
        assert!(result.contains("\x1b]777;emterm;markdown;begin"));
        assert!(result.contains(&format!("id={}", session_id)));
        assert!(result.contains("format=gfm"));
        assert!(result.contains("render=fullscreen"));
        assert!(result.contains("version=1.0"));
        assert!(result.contains("\x1b]777;emterm;markdown;chunk"));
        assert!(result.contains("seq=0"));
        assert!(result.contains("data=SGVsbG8="));
        assert!(result.contains("\x1b]777;emterm;markdown;end"));
    }

    #[test]
    fn test_generate_markdown_osc_multiple_chunks() {
        let session_id = Uuid::new_v4();
        let chunks = vec![
            "chunk0".to_string(),
            "chunk1".to_string(),
            "chunk2".to_string(),
        ];

        let result = generate_markdown_osc(&session_id, chunks);

        // Verify sequential chunk numbers
        assert!(result.contains("seq=0"));
        assert!(result.contains("seq=1"));
        assert!(result.contains("seq=2"));
        assert!(result.contains("data=chunk0"));
        assert!(result.contains("data=chunk1"));
        assert!(result.contains("data=chunk2"));
    }

    #[test]
    fn test_generate_markdown_osc_uuid_consistency() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["data".to_string()];

        let result = generate_markdown_osc(&session_id, chunks);

        // UUID should appear in begin, chunk, and end sequences
        let uuid_str = session_id.to_string();
        let uuid_count = result.matches(&uuid_str).count();
        assert_eq!(uuid_count, 3); // begin, chunk, end
    }

    #[test]
    fn test_generate_download_osc_single_chunk() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["SGVsbG8=".to_string()];

        let result = generate_download_osc(&session_id, "test.txt", 5, chunks);

        assert!(result.contains("\x1b]777;emterm;download;begin"));
        assert!(result.contains(&format!("id={}", session_id)));
        assert!(result.contains("name=test.txt"));
        assert!(result.contains("size=5"));
        assert!(result.contains("version=1.0"));
        assert!(result.contains("\x1b]777;emterm;download;chunk"));
        assert!(result.contains("seq=0"));
        assert!(result.contains("data=SGVsbG8="));
        assert!(result.contains("\x1b]777;emterm;download;end"));
    }

    #[test]
    fn test_generate_download_osc_multiple_chunks() {
        let session_id = Uuid::new_v4();
        let chunks = vec![
            "chunk0".to_string(),
            "chunk1".to_string(),
            "chunk2".to_string(),
        ];

        let result = generate_download_osc(&session_id, "data.bin", 100, chunks);

        assert!(result.contains("seq=0"));
        assert!(result.contains("seq=1"));
        assert!(result.contains("seq=2"));
        assert!(result.contains("data=chunk0"));
        assert!(result.contains("data=chunk1"));
        assert!(result.contains("data=chunk2"));
    }

    #[test]
    fn test_generate_download_osc_empty_chunks() {
        let session_id = Uuid::new_v4();
        let chunks = vec![];

        let result = generate_download_osc(&session_id, "empty.txt", 0, chunks);

        assert!(result.contains("\x1b]777;emterm;download;begin"));
        assert!(result.contains("size=0"));
        assert!(result.contains("\x1b]777;emterm;download;end"));
        assert!(!result.contains("\x1b]777;emterm;download;chunk"));
    }

    #[test]
    fn test_generate_download_osc_uuid_consistency() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["data".to_string()];

        let result = generate_download_osc(&session_id, "file.txt", 4, chunks);

        let uuid_str = session_id.to_string();
        let uuid_count = result.matches(&uuid_str).count();
        assert_eq!(uuid_count, 3); // begin, chunk, end
    }

    #[test]
    fn test_generate_download_osc_sanitizes_semicolons_in_filename() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["data".to_string()];

        let result = generate_download_osc(&session_id, "evil;inject=val.txt", 4, chunks);

        // Semicolons should be stripped from the filename
        assert!(result.contains("name=evilinject=val.txt"));
        assert!(!result.contains("name=evil;inject=val.txt"));
    }

    // --- Individual download OSC generator tests ---

    #[test]
    fn test_generate_download_osc_begin_format() {
        let session_id = Uuid::new_v4();
        let result = generate_download_osc_begin(&session_id, "test.txt", 12345);
        assert!(result.starts_with("\x1b]777;emterm;download;begin;"));
        assert!(result.contains(&format!("id={}", session_id)));
        assert!(result.contains("name=test.txt"));
        assert!(result.contains("size=12345"));
        assert!(result.contains("version=1.0"));
        assert!(result.ends_with("\x1b\\"));
    }

    #[test]
    fn test_generate_download_osc_begin_sanitizes_filename() {
        let session_id = Uuid::new_v4();
        let result = generate_download_osc_begin(&session_id, "evil;inject.txt", 100);
        assert!(result.contains("name=evilinject.txt"));
        assert!(!result.contains("name=evil;inject.txt"));
    }

    #[test]
    fn test_generate_download_osc_chunk_format() {
        let session_id = Uuid::new_v4();
        let result = generate_download_osc_chunk(&session_id, 5, "SGVsbG8=");
        assert!(result.starts_with("\x1b]777;emterm;download;chunk;"));
        assert!(result.contains(&format!("id={}", session_id)));
        assert!(result.contains("seq=5"));
        assert!(result.contains("data=SGVsbG8="));
        assert!(result.ends_with("\x1b\\"));
    }

    #[test]
    fn test_generate_download_osc_end_format() {
        let session_id = Uuid::new_v4();
        let result = generate_download_osc_end(&session_id);
        assert!(result.starts_with("\x1b]777;emterm;download;end;"));
        assert!(result.contains(&format!("id={}", session_id)));
        assert!(result.ends_with("\x1b\\"));
    }

    #[test]
    fn test_individual_generators_match_combined() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["chunk0data".to_string(), "chunk1data".to_string()];
        let combined = generate_download_osc(&session_id, "file.bin", 100, chunks.clone());

        let mut individual = String::new();
        individual.push_str(&generate_download_osc_begin(&session_id, "file.bin", 100));
        for (i, data) in chunks.iter().enumerate() {
            individual.push_str(&generate_download_osc_chunk(&session_id, i, data));
        }
        individual.push_str(&generate_download_osc_end(&session_id));

        assert_eq!(combined, individual);
    }

    // --- JSON OSC generator tests ---

    #[test]
    fn test_generate_json_osc_single_chunk() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["eyJrZXkiOiJ2YWx1ZSJ9".to_string()];

        let result = generate_json_osc(&session_id, chunks);

        assert!(result.contains("\x1b]777;emterm;json;begin"));
        assert!(result.contains(&format!("id={}", session_id)));
        assert!(result.contains("version=1.0"));
        assert!(result.contains("\x1b]777;emterm;json;chunk"));
        assert!(result.contains("seq=0"));
        assert!(result.contains("data=eyJrZXkiOiJ2YWx1ZSJ9"));
        assert!(result.contains("\x1b]777;emterm;json;end"));
    }

    #[test]
    fn test_generate_json_osc_multiple_chunks() {
        let session_id = Uuid::new_v4();
        let chunks = vec![
            "chunk0".to_string(),
            "chunk1".to_string(),
        ];

        let result = generate_json_osc(&session_id, chunks);

        assert!(result.contains("seq=0"));
        assert!(result.contains("seq=1"));
        assert!(result.contains("data=chunk0"));
        assert!(result.contains("data=chunk1"));
    }

    #[test]
    fn test_generate_json_osc_uuid_consistency() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["data".to_string()];

        let result = generate_json_osc(&session_id, chunks);

        let uuid_str = session_id.to_string();
        let uuid_count = result.matches(&uuid_str).count();
        assert_eq!(uuid_count, 3); // begin, chunk, end
    }

    #[test]
    fn test_generate_json_osc_empty_chunks() {
        let session_id = Uuid::new_v4();
        let chunks = vec![];

        let result = generate_json_osc(&session_id, chunks);

        assert!(result.contains("\x1b]777;emterm;json;begin"));
        assert!(result.contains("\x1b]777;emterm;json;end"));
        assert!(!result.contains("\x1b]777;emterm;json;chunk"));
    }

    // --- YAML OSC generator tests ---

    #[test]
    fn test_generate_yaml_osc_single_chunk() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["a2V5OiB2YWx1ZQ==".to_string()];

        let result = generate_yaml_osc(&session_id, chunks);

        assert!(result.contains("\x1b]777;emterm;yaml;begin"));
        assert!(result.contains(&format!("id={}", session_id)));
        assert!(result.contains("version=1.0"));
        assert!(result.contains("\x1b]777;emterm;yaml;chunk"));
        assert!(result.contains("seq=0"));
        assert!(result.contains("\x1b]777;emterm;yaml;end"));
    }

    #[test]
    fn test_generate_yaml_osc_uuid_consistency() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["data".to_string()];

        let result = generate_yaml_osc(&session_id, chunks);

        let uuid_str = session_id.to_string();
        let uuid_count = result.matches(&uuid_str).count();
        assert_eq!(uuid_count, 3); // begin, chunk, end
    }

    #[test]
    fn test_generate_markdown_osc_empty_chunks() {
        let session_id = Uuid::new_v4();
        let chunks = vec![];

        let result = generate_markdown_osc(&session_id, chunks);

        // Should still have begin and end sequences
        assert!(result.contains("\x1b]777;emterm;markdown;begin"));
        assert!(result.contains("\x1b]777;emterm;markdown;end"));
        // Should not have any chunk sequences
        assert!(!result.contains("\x1b]777;emterm;markdown;chunk"));
    }
}
