//! Duplicate file checking via sftp `ls` output parsing.
//!
//! Parses the output of `sftp ls` to determine which files
//! already exist on the remote host.

/// Parse sftp `ls` output to extract file/directory names.
///
/// sftp `ls` with a directory argument outputs full paths (e.g., `/tmp/file.txt`)
/// with trailing whitespace padding. We extract the basename from each entry.
pub fn parse_ls_output(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            // Skip sftp prompt lines (e.g., "sftp> ls ..." or "sftp> bye")
            if trimmed.starts_with("sftp>") {
                return None;
            }

            // Extract basename: sftp outputs full paths like "/tmp/file.txt"
            let name = match trimmed.rfind('/') {
                Some(pos) => &trimmed[pos + 1..],
                None => trimmed,
            };

            // Skip empty names (trailing slash), "." and ".."
            if name.is_empty() || name == "." || name == ".." {
                return None;
            }

            Some(name.to_string())
        })
        .collect()
}

/// Find which of the given file names already exist in the remote directory.
///
/// Compares the `file_names` against the parsed `ls_output` to find conflicts.
pub fn find_duplicates(ls_output: &str, file_names: &[String]) -> Vec<String> {
    let remote_files = parse_ls_output(ls_output);
    file_names
        .iter()
        .filter(|name| remote_files.iter().any(|rf| rf == *name))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ls_output_simple() {
        let output = "file1.txt\nfile2.txt\ndir1\n";
        let names = parse_ls_output(output);
        assert_eq!(names, vec!["file1.txt", "file2.txt", "dir1"]);
    }

    #[test]
    fn test_parse_ls_output_full_paths() {
        // Actual sftp output: full paths with trailing whitespace padding
        let output = concat!(
            "sftp> ls \"/tmp\"\n",
            "/tmp/archive.zip                                 \n",
            "/tmp/session-1000                                \n",
            "/tmp/report.txt                                  \n",
            "sftp> bye\n",
        );
        let names = parse_ls_output(output);
        assert_eq!(names, vec!["archive.zip", "session-1000", "report.txt"]);
    }

    #[test]
    fn test_parse_ls_output_full_paths_with_spaces_in_name() {
        let output = concat!(
            "/tmp/My Document 2026.pdf                                       \n",
            "/tmp/normal.txt                                                 \n",
        );
        let names = parse_ls_output(output);
        assert_eq!(names, vec!["My Document 2026.pdf", "normal.txt"]);
    }

    #[test]
    fn test_parse_ls_output_skips_dots() {
        let output = ".\n..\nfile.txt\n";
        let names = parse_ls_output(output);
        assert_eq!(names, vec!["file.txt"]);
    }

    #[test]
    fn test_parse_ls_output_skips_dots_full_path() {
        let output = "/tmp/.\n/tmp/..\n/tmp/file.txt\n";
        let names = parse_ls_output(output);
        assert_eq!(names, vec!["file.txt"]);
    }

    #[test]
    fn test_parse_ls_output_skips_sftp_prompt() {
        let output = "sftp> ls\nfile.txt\nsftp> \n";
        let names = parse_ls_output(output);
        assert_eq!(names, vec!["file.txt"]);
    }

    #[test]
    fn test_parse_ls_output_empty() {
        let output = "";
        let names = parse_ls_output(output);
        assert!(names.is_empty());
    }

    #[test]
    fn test_parse_ls_output_whitespace_only() {
        let output = "  \n  \n";
        let names = parse_ls_output(output);
        assert!(names.is_empty());
    }

    #[test]
    fn test_find_duplicates_some_conflicts() {
        let output = "/home/user/file1.txt\n/home/user/file2.txt\n/home/user/dir1\n";
        let file_names = vec![
            "file1.txt".to_string(),
            "newfile.txt".to_string(),
            "dir1".to_string(),
        ];
        let duplicates = find_duplicates(output, &file_names);
        assert_eq!(duplicates, vec!["file1.txt", "dir1"]);
    }

    #[test]
    fn test_find_duplicates_no_conflicts() {
        let output = "/home/user/existing.txt\n";
        let file_names = vec!["new1.txt".to_string(), "new2.txt".to_string()];
        let duplicates = find_duplicates(output, &file_names);
        assert!(duplicates.is_empty());
    }

    #[test]
    fn test_find_duplicates_all_conflicts() {
        let output = "/dir/a.txt\n/dir/b.txt\n";
        let file_names = vec!["a.txt".to_string(), "b.txt".to_string()];
        let duplicates = find_duplicates(output, &file_names);
        assert_eq!(duplicates, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn test_find_duplicates_empty_remote() {
        let output = "";
        let file_names = vec!["file.txt".to_string()];
        let duplicates = find_duplicates(output, &file_names);
        assert!(duplicates.is_empty());
    }

    #[test]
    fn test_find_duplicates_empty_local() {
        let output = "/dir/file.txt\n";
        let file_names: Vec<String> = vec![];
        let duplicates = find_duplicates(output, &file_names);
        assert!(duplicates.is_empty());
    }

    #[test]
    fn test_find_duplicates_with_spaces_in_filename() {
        let output = concat!(
            "sftp> ls \"/upload\"\n",
            "/upload/My Document 2026.pdf                                    \n",
            "/upload/other.txt                                               \n",
            "sftp> bye\n",
        );
        let file_names = vec!["My Document 2026.pdf".to_string()];
        let duplicates = find_duplicates(output, &file_names);
        assert_eq!(duplicates, vec!["My Document 2026.pdf"]);
    }
}
