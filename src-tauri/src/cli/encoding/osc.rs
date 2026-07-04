//! OSC 777 frame builders for markdown / json / yaml.
//!
//! Ported from `src-tauri/src/encoding/osc.rs`. The download / image-response
//! / image-error builders from the source are intentionally omitted —
//! Phase A + B does not include the `download` subcommand or the
//! markdown interactive image-response path.

use uuid::Uuid;

/// Sanitize a string for safe embedding in OSC parameter values.
/// Removes semicolons (OSC field delimiter) and control characters.
fn sanitize_osc_value(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c != ';' && !c.is_control())
        .collect()
}

/// Generates OSC 777 sequences for Markdown content.
pub fn generate_markdown_osc(
    session_id: &Uuid,
    chunks: Vec<String>,
    basedir: Option<&str>,
    interactive: bool,
) -> String {
    let total_data: usize = chunks.iter().map(|c| c.len()).sum();
    let header_overhead = 100;
    let estimated = total_data + header_overhead * (chunks.len() + 2);
    let mut output = String::with_capacity(estimated);
    let id = session_id.to_string();

    let basedir_param = match basedir {
        Some(dir) => format!(";basedir={}", sanitize_osc_value(dir)),
        None => String::new(),
    };
    output.push_str(&format!(
        "\x1b]777;emterm;markdown;begin;id={};format=gfm;render=fullscreen;version=1.0{}\x1b\\",
        id, basedir_param
    ));

    for (seq, data) in chunks.iter().enumerate() {
        output.push_str(&format!(
            "\x1b]777;emterm;markdown;chunk;id={};seq={};data={}\x1b\\",
            id, seq, data
        ));
    }

    let interactive_param = if interactive { ";interactive=1" } else { "" };
    output.push_str(&format!(
        "\x1b]777;emterm;markdown;end;id={}{}\x1b\\",
        id, interactive_param
    ));

    output
}

/// Generates OSC 777 sequences for JSON content.
pub fn generate_json_osc(session_id: &Uuid, chunks: Vec<String>) -> String {
    let total_data: usize = chunks.iter().map(|c| c.len()).sum();
    let header_overhead = 100;
    let estimated = total_data + header_overhead * (chunks.len() + 2);
    let mut output = String::with_capacity(estimated);
    let id = session_id.to_string();

    output.push_str(&format!(
        "\x1b]777;emterm;json;begin;id={};version=1.0\x1b\\",
        id
    ));

    for (seq, data) in chunks.iter().enumerate() {
        output.push_str(&format!(
            "\x1b]777;emterm;json;chunk;id={};seq={};data={}\x1b\\",
            id, seq, data
        ));
    }

    output.push_str(&format!("\x1b]777;emterm;json;end;id={}\x1b\\", id));

    output
}

/// Generates OSC 777 sequences for YAML content.
pub fn generate_yaml_osc(session_id: &Uuid, chunks: Vec<String>) -> String {
    let total_data: usize = chunks.iter().map(|c| c.len()).sum();
    let header_overhead = 100;
    let estimated = total_data + header_overhead * (chunks.len() + 2);
    let mut output = String::with_capacity(estimated);
    let id = session_id.to_string();

    output.push_str(&format!(
        "\x1b]777;emterm;yaml;begin;id={};version=1.0\x1b\\",
        id
    ));

    for (seq, data) in chunks.iter().enumerate() {
        output.push_str(&format!(
            "\x1b]777;emterm;yaml;chunk;id={};seq={};data={}\x1b\\",
            id, seq, data
        ));
    }

    output.push_str(&format!("\x1b]777;emterm;yaml;end;id={}\x1b\\", id));

    output
}

/// Generates OSC 777 sequences for HTML content.
///
/// Same verb grammar as [`generate_markdown_osc`] (begin / chunk / end),
/// but without the `format`/`render`/`interactive` params — the HTML
/// viewer renders the raw document with its own styles only
/// (feature-docs/html-viewer/IMPLEMENTATION.md, shared-component table).
pub fn generate_html_osc(session_id: &Uuid, chunks: Vec<String>, basedir: Option<&str>) -> String {
    let total_data: usize = chunks.iter().map(|c| c.len()).sum();
    let header_overhead = 100;
    let estimated = total_data + header_overhead * (chunks.len() + 2);
    let mut output = String::with_capacity(estimated);
    let id = session_id.to_string();

    let basedir_param = match basedir {
        Some(dir) => format!(";basedir={}", sanitize_osc_value(dir)),
        None => String::new(),
    };
    output.push_str(&format!(
        "\x1b]777;emterm;html;begin;id={};version=1.0{}\x1b\\",
        id, basedir_param
    ));

    for (seq, data) in chunks.iter().enumerate() {
        output.push_str(&format!(
            "\x1b]777;emterm;html;chunk;id={};seq={};data={}\x1b\\",
            id, seq, data
        ));
    }

    output.push_str(&format!("\x1b]777;emterm;html;end;id={}\x1b\\", id));

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_markdown_osc_single_chunk() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["SGVsbG8=".to_string()];

        let result = generate_markdown_osc(&session_id, chunks, None, false);

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

        let result = generate_markdown_osc(&session_id, chunks, None, false);

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

        let result = generate_markdown_osc(&session_id, chunks, None, false);

        let uuid_str = session_id.to_string();
        let uuid_count = result.matches(&uuid_str).count();
        assert_eq!(uuid_count, 3);
    }

    #[test]
    fn test_generate_markdown_osc_empty_chunks() {
        let session_id = Uuid::new_v4();
        let chunks = vec![];

        let result = generate_markdown_osc(&session_id, chunks, None, false);

        assert!(result.contains("\x1b]777;emterm;markdown;begin"));
        assert!(result.contains("\x1b]777;emterm;markdown;end"));
        assert!(!result.contains("\x1b]777;emterm;markdown;chunk"));
    }

    #[test]
    fn test_generate_markdown_osc_interactive_flag_on_end_only() {
        let session_id = Uuid::new_v4();
        let id = session_id.to_string();
        let chunks = vec!["SGVsbG8=".to_string()];

        let interactive = generate_markdown_osc(&session_id, chunks.clone(), None, true);
        assert!(interactive.contains(&format!(
            "\x1b]777;emterm;markdown;end;id={};interactive=1\x1b\\",
            id
        )));
        assert_eq!(interactive.matches("interactive=1").count(), 1);
        for seq in interactive.split("\x1b\\") {
            if seq.contains("markdown;begin") || seq.contains("markdown;chunk") {
                assert!(
                    !seq.contains("interactive=1"),
                    "begin/chunk sequence carried interactive flag: {seq:?}"
                );
            }
        }
        assert!(interactive.contains("\x1b]777;emterm;markdown;begin"));
        assert!(interactive.contains("\x1b]777;emterm;markdown;chunk"));

        let non_interactive = generate_markdown_osc(&session_id, chunks, None, false);
        assert!(!non_interactive.contains("interactive=1"));
        assert!(non_interactive.contains(&format!("\x1b]777;emterm;markdown;end;id={}\x1b\\", id)));
    }

    #[test]
    fn test_generate_markdown_osc_with_basedir() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["SGVsbG8=".to_string()];

        let result = generate_markdown_osc(&session_id, chunks, Some("/home/user/docs"), false);

        assert!(result.contains("basedir=/home/user/docs"));
        assert!(result.contains("format=gfm"));
        assert!(result.contains("render=fullscreen"));
        assert!(result.contains("version=1.0"));
    }

    #[test]
    fn test_generate_markdown_osc_without_basedir_backward_compatible() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["SGVsbG8=".to_string()];

        let result = generate_markdown_osc(&session_id, chunks, None, false);

        assert!(!result.contains("basedir="));
        assert!(result.contains("format=gfm"));
        assert!(result.contains("render=fullscreen"));
    }

    #[test]
    fn test_generate_markdown_osc_basedir_sanitizes_semicolons() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["data".to_string()];

        let result = generate_markdown_osc(&session_id, chunks, Some("/path;evil/dir"), false);

        assert!(result.contains("basedir=/pathevil/dir"));
        assert!(!result.contains("basedir=/path;evil/dir"));
    }

    #[test]
    fn test_generate_markdown_osc_basedir_sanitizes_control_chars() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["data".to_string()];

        let result = generate_markdown_osc(&session_id, chunks, Some("/path/\x1b[0m/dir"), false);

        assert!(result.contains("basedir=/path/[0m/dir"));
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
        let chunks = vec!["chunk0".to_string(), "chunk1".to_string()];

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
        assert_eq!(uuid_count, 3);
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
        assert_eq!(uuid_count, 3);
    }

    // --- HTML OSC generator tests ---
    // References AC-1 (html-viewer task0001): begin -> chunk(s, seq-ordered)
    // -> end, kind=html, with/without basedir, and basedir sanitization.

    #[test]
    fn test_generate_html_osc_single_chunk() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["PGh0bWw+".to_string()];

        let result = generate_html_osc(&session_id, chunks, None);

        assert!(result.contains("\x1b]777;emterm;html;begin"));
        assert!(result.contains(&format!("id={}", session_id)));
        assert!(result.contains("version=1.0"));
        assert!(result.contains("\x1b]777;emterm;html;chunk"));
        assert!(result.contains("seq=0"));
        assert!(result.contains("data=PGh0bWw+"));
        assert!(result.contains("\x1b]777;emterm;html;end"));
    }

    #[test]
    fn test_generate_html_osc_multiple_chunks_are_seq_ordered() {
        let session_id = Uuid::new_v4();
        let chunks = vec![
            "chunk0".to_string(),
            "chunk1".to_string(),
            "chunk2".to_string(),
        ];

        let result = generate_html_osc(&session_id, chunks, None);

        let begin_pos = result.find("html;begin").unwrap();
        let chunk0_pos = result.find("seq=0;data=chunk0").unwrap();
        let chunk1_pos = result.find("seq=1;data=chunk1").unwrap();
        let chunk2_pos = result.find("seq=2;data=chunk2").unwrap();
        let end_pos = result.find("html;end").unwrap();
        assert!(begin_pos < chunk0_pos);
        assert!(chunk0_pos < chunk1_pos);
        assert!(chunk1_pos < chunk2_pos);
        assert!(chunk2_pos < end_pos);
    }

    #[test]
    fn test_generate_html_osc_uuid_consistency() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["data".to_string()];

        let result = generate_html_osc(&session_id, chunks, None);

        let uuid_str = session_id.to_string();
        let uuid_count = result.matches(&uuid_str).count();
        assert_eq!(uuid_count, 3);
    }

    #[test]
    fn test_generate_html_osc_empty_chunks_skips_chunk_frame() {
        let session_id = Uuid::new_v4();
        let chunks = vec![];

        let result = generate_html_osc(&session_id, chunks, None);

        assert!(result.contains("\x1b]777;emterm;html;begin"));
        assert!(result.contains("\x1b]777;emterm;html;end"));
        assert!(!result.contains("\x1b]777;emterm;html;chunk"));
    }

    #[test]
    fn test_generate_html_osc_with_basedir() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["PGh0bWw+".to_string()];

        let result = generate_html_osc(&session_id, chunks, Some("/home/user/docs"));

        assert!(result.contains("basedir=/home/user/docs"));
        assert!(result.contains("\x1b]777;emterm;html;begin"));
    }

    #[test]
    fn test_generate_html_osc_without_basedir() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["PGh0bWw+".to_string()];

        let result = generate_html_osc(&session_id, chunks, None);

        assert!(!result.contains("basedir="));
    }

    #[test]
    fn test_generate_html_osc_basedir_sanitizes_semicolons() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["data".to_string()];

        let result = generate_html_osc(&session_id, chunks, Some("/path;evil/dir"));

        assert!(result.contains("basedir=/pathevil/dir"));
        assert!(!result.contains("basedir=/path;evil/dir"));
    }

    #[test]
    fn test_generate_html_osc_basedir_sanitizes_control_chars() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["data".to_string()];

        let result = generate_html_osc(&session_id, chunks, Some("/path/\x1b[0m/dir"));

        assert!(result.contains("basedir=/path/[0m/dir"));
    }

    #[test]
    fn test_generate_html_osc_kind_is_html_not_markdown() {
        let session_id = Uuid::new_v4();
        let chunks = vec!["data".to_string()];

        let result = generate_html_osc(&session_id, chunks, None);

        assert!(!result.contains("emterm;markdown"));
        assert!(!result.contains("format=gfm"));
        assert!(!result.contains("render=fullscreen"));
    }
}
