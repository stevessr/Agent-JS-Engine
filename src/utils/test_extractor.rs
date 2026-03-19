use std::fs;
use std::path::PathBuf;

pub struct TestInfo {
    pub source: String,
    pub expected_parse_error: bool,
}

pub fn extract_test(path: &PathBuf) -> TestInfo {
    let source = fs::read_to_string(path).unwrap_or_default();
    
    // A quick n dirty check for negative phase
    let expected_parse_error = source.contains("negative:") && source.contains("phase: parse");
    
    TestInfo {
        source,
        expected_parse_error,
    }
}
