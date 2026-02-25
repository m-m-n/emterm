use uuid::Uuid;

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
    output.push_str(&format!(
        "\x1b]777;emterm;markdown;end;id={}\x1b\\",
        id
    ));

    output
}

#[cfg(test)]
mod tests {
    use super::*;

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
