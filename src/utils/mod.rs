//! Utilities and definitions.

use std::fs;
use std::path::PathBuf;

pub struct TestInfo {
    pub source: String,
    pub expected_parse_error: bool,
    pub expected_runtime_error: bool,
}

pub fn extract_test(path: &PathBuf) -> TestInfo {
    let source = fs::read_to_string(path).unwrap_or_default();

    // Very coarse fast-path checking for Test262 YAML frontmatter
    let expected_parse_error = source.contains("negative:") && source.contains("phase: parse");
    let expected_runtime_error = source.contains("negative:")
        && (source.contains("phase: runtime") || source.contains("phase: resolution"));

    TestInfo {
        source,
        expected_parse_error,
        expected_runtime_error,
    }
}
