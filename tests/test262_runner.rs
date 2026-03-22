use ai_agent::engine::{EngineError, EvalOptions, JsEngine};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const DEFAULT_TEST262_DIR: &str = "test262";
const LOOP_ITERATION_LIMIT: u64 = 5_000_000;
const PROGRESS_INTERVAL: usize = 2_000;
const SAMPLE_LIMIT: usize = 12;
const UNSUPPORTED_FEATURES: &[&str] = &["source-phase-imports", "import-defer"];

#[derive(Debug, Clone, Default, Deserialize)]
struct Test262Metadata {
    #[serde(default)]
    includes: Vec<String>,
    #[serde(default)]
    flags: Vec<String>,
    #[serde(default)]
    features: Vec<String>,
    #[serde(default)]
    negative: Option<NegativeMetadata>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct NegativeMetadata {
    phase: Option<String>,
    #[serde(rename = "type")]
    error_type: Option<String>,
}

#[derive(Debug, Clone)]
struct TestCase {
    path: PathBuf,
    source: String,
    metadata: Test262Metadata,
}

#[derive(Debug, Clone)]
struct HarnessCache {
    files: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone)]
struct CaseResult {
    outcome: Outcome,
    reason: Option<String>,
}

impl Test262Metadata {
    fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|value| value == flag)
    }

    fn has_feature(&self, feature: &str) -> bool {
        self.features.iter().any(|value| value == feature)
    }
}

impl HarnessCache {
    fn load(root: &Path) -> Self {
        let mut files = HashMap::new();

        for entry in WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let Ok(contents) = fs::read_to_string(entry.path()) else {
                continue;
            };

            let key = entry
                .path()
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string();
            files.insert(key, contents);
        }

        Self { files }
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.files.get(name).map(String::as_str)
    }
}

fn extract_metadata(source: &str) -> Test262Metadata {
    let Some(frontmatter_start) = source.find("/*---") else {
        return Test262Metadata::default();
    };
    let Some(frontmatter_end) = source[frontmatter_start + 5..].find("---*/") else {
        return Test262Metadata::default();
    };
    let yaml = &source[frontmatter_start + 5..frontmatter_start + 5 + frontmatter_end];
    serde_yaml::from_str(yaml).unwrap_or_default()
}

fn discover_cases(test_root: &Path) -> Vec<TestCase> {
    let filter = std::env::var("TEST262_FILTER").ok();
    let max_cases = std::env::var("TEST262_MAX_CASES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok());

    let mut cases = WalkDir::new(test_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("js"))
        .filter(|entry| {
            entry
                .path()
                .file_name()
                .and_then(|value| value.to_str())
                .is_none_or(|name| !name.contains("_FIXTURE"))
        })
        .filter(|entry| {
            filter
                .as_ref()
                .is_none_or(|needle| entry.path().to_string_lossy().contains(needle))
        })
        .filter_map(|entry| {
            let path = entry.into_path();
            let source = fs::read_to_string(&path).ok()?;
            let metadata = extract_metadata(&source);
            Some(TestCase {
                path,
                source,
                metadata,
            })
        })
        .collect::<Vec<_>>();

    cases.sort_by(|left, right| left.path.cmp(&right.path));
    if let Some(max_cases) = max_cases {
        cases.truncate(max_cases);
    }
    cases
}

fn skip_reason(case: &TestCase, suite_root: &Path) -> Option<&'static str> {
    let relative = case
        .path
        .strip_prefix(suite_root)
        .unwrap_or(case.path.as_path());

    if relative.starts_with("test/staging") {
        return Some("staging");
    }
    if relative.starts_with("test/intl402") {
        return Some("intl402");
    }
    if relative.starts_with("test/built-ins/Temporal") {
        return Some("temporal");
    }
    for feature in UNSUPPORTED_FEATURES {
        if case.metadata.has_feature(feature) && !supports_feature_case(relative, feature) {
            return Some(feature);
        }
    }
    if case.metadata.has_feature("import-attributes")
        && !supports_import_attributes_case(relative, &case.metadata)
    {
        return Some("import-attributes");
    }
    None
}

fn supports_feature_case(relative: &Path, feature: &str) -> bool {
    match feature {
        "import-defer" => supports_import_defer_case(relative),
        "source-phase-imports" => supports_source_phase_import_case(relative),
        _ => false,
    }
}

fn supports_import_defer_case(relative: &Path) -> bool {
    is_import_defer_dynamic_catch_case(relative)
        || is_import_defer_dynamic_valid_syntax_case(relative)
        || is_import_defer_static_syntax_case(relative)
}

fn is_import_defer_dynamic_catch_case(relative: &Path) -> bool {
    relative.starts_with("test/language/expressions/dynamic-import/catch")
        && relative
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.contains("import-defer-specifier-tostring-abrupt-rejects"))
}

fn is_import_defer_dynamic_valid_syntax_case(relative: &Path) -> bool {
    relative.starts_with("test/language/expressions/dynamic-import/syntax/valid")
        && relative
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.contains("import-defer"))
}

fn is_import_defer_static_syntax_case(relative: &Path) -> bool {
    matches!(
        relative,
        path if path == Path::new("test/language/import/import-defer/syntax/valid-defer-namespace.js")
            || path == Path::new("test/language/import/import-defer/syntax/valid-default-binding-named-defer.js")
            || path == Path::new("test/language/import/import-defer/syntax/import-attributes.js")
    ) || relative.starts_with("test/language/import/import-defer/errors/syntax-error")
}

fn supports_source_phase_import_case(relative: &Path) -> bool {
    relative.starts_with("test/built-ins/AbstractModuleSource")
        || relative == Path::new("test/language/module-code/source-phase-import/import-source.js")
        || relative.starts_with("test/language/expressions/dynamic-import/catch")
        || is_valid_dynamic_import_source_syntax_case(relative)
}

fn is_valid_dynamic_import_source_syntax_case(relative: &Path) -> bool {
    relative.starts_with("test/language/expressions/dynamic-import/syntax/valid")
        && relative
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.contains("import-source"))
}

fn supports_import_attributes_case(relative: &Path, metadata: &Test262Metadata) -> bool {
    if metadata.has_feature("source-phase-imports") || metadata.has_feature("import-defer") {
        return false;
    }

    relative.starts_with("test/language/expressions/dynamic-import")
        || relative.starts_with("test/language/import/import-attributes")
        || relative.starts_with("test/language/import/import-bytes")
}

fn build_source(case: &TestCase, harness: &HarnessCache) -> String {
    if case.metadata.has_flag("raw") {
        return case.source.clone();
    }

    if matches!(
        case.metadata
            .negative
            .as_ref()
            .and_then(|negative| negative.phase.as_deref()),
        Some("parse")
    ) {
        return case.source.clone();
    }

    let mut include_order = vec!["sta.js".to_string(), "assert.js".to_string()];
    if case.metadata.has_flag("async") {
        include_order.push("doneprintHandle.js".to_string());
    }
    include_order.extend(case.metadata.includes.iter().cloned());

    let mut seen = HashSet::new();
    let mut combined = String::new();

    for include in include_order {
        if !seen.insert(include.clone()) {
            continue;
        }
        if let Some(contents) = harness.get(&include) {
            combined.push_str(contents);
            combined.push('\n');
        }
    }

    combined.push_str(&case.source);
    combined
}

fn expected_error_matches(expected: Option<&str>, actual: &EngineError) -> bool {
    expected.is_none_or(|name| name == actual.name)
}

fn run_case(case: &TestCase, harness: &HarnessCache, suite_root: &Path) -> CaseResult {
    if let Some(reason) = skip_reason(case, suite_root) {
        return CaseResult {
            outcome: Outcome::Skipped,
            reason: Some(reason.to_string()),
        };
    }

    let engine = JsEngine::new();
    let source = build_source(case, harness);
    let options = EvalOptions {
        strict: case.metadata.has_flag("onlyStrict"),
        bootstrap_test262: !case.metadata.has_flag("raw"),
        loop_iteration_limit: Some(LOOP_ITERATION_LIMIT),
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        if case.metadata.has_flag("module") {
            engine.eval_module_with_options(&source, &case.path, suite_root, &options)
        } else {
            engine.eval_script_with_options(&source, &case.path, suite_root, &options)
        }
    }));
    let result = match result {
        Ok(result) => result,
        Err(payload) => {
            let reason = if let Some(message) = payload.downcast_ref::<&str>() {
                format!("panic: {message}")
            } else if let Some(message) = payload.downcast_ref::<String>() {
                format!("panic: {message}")
            } else {
                "panic: non-string payload".to_string()
            };

            return CaseResult {
                outcome: Outcome::Failed,
                reason: Some(reason),
            };
        }
    };

    let outcome = match (&case.metadata.negative, result) {
        (Some(negative), Err(error)) => {
            if expected_error_matches(negative.error_type.as_deref(), &error) {
                Outcome::Passed
            } else {
                Outcome::Failed
            }
        }
        (Some(_), Ok(_)) => Outcome::Failed,
        (None, Err(_)) => Outcome::Failed,
        (None, Ok(output)) if case.metadata.has_flag("async") => {
            let has_failure = output
                .printed
                .iter()
                .any(|line| line.starts_with("Test262:AsyncTestFailure:"));
            let has_completion = output
                .printed
                .iter()
                .any(|line| line == "Test262:AsyncTestComplete");
            if !has_failure && has_completion {
                Outcome::Passed
            } else {
                Outcome::Failed
            }
        }
        (None, Ok(_)) => Outcome::Passed,
    };

    CaseResult {
        outcome,
        reason: match outcome {
            Outcome::Failed => Some("assertion or runtime mismatch".to_string()),
            _ => None,
        },
    }
}

#[test]
fn test262_metadata_smoke() {
    let source = r#"/*---
includes: [compareArray.js]
flags: [onlyStrict, async]
negative:
  phase: runtime
  type: TypeError
---*/
42;
"#;
    let metadata = extract_metadata(source);

    assert!(metadata.has_flag("onlyStrict"));
    assert!(metadata.has_flag("async"));
    assert_eq!(metadata.includes, vec!["compareArray.js"]);
    assert_eq!(
        metadata
            .negative
            .as_ref()
            .and_then(|negative| negative.error_type.as_deref()),
        Some("TypeError")
    );
}

#[test]
#[ignore = "long-running test262 sweep"]
fn test262_core_profile() {
    let filter = std::env::var("TEST262_FILTER").ok();
    let max_cases = std::env::var("TEST262_MAX_CASES").ok();
    let suite_root = PathBuf::from(
        std::env::var("TEST262_DIR").unwrap_or_else(|_| DEFAULT_TEST262_DIR.to_string()),
    );
    let test_root = suite_root.join("test");
    let harness_root = suite_root.join("harness");

    if !test_root.exists() || !harness_root.exists() {
        eprintln!(
            "test262 suite not found. Set TEST262_DIR or run ./run_test262.sh to fetch it first."
        );
        return;
    }

    let harness = HarnessCache::load(&harness_root);
    let cases = discover_cases(&test_root);

    let mut total = 0usize;
    let mut executed = 0usize;
    let mut passed = 0usize;
    let mut skipped = 0usize;
    let mut skip_reasons = BTreeMap::<String, usize>::new();
    let mut samples = Vec::new();

    for (index, case) in cases.iter().enumerate() {
        total += 1;
        let result = run_case(case, &harness, &suite_root);
        match result.outcome {
            Outcome::Passed => {
                executed += 1;
                passed += 1;
            }
            Outcome::Failed => {
                executed += 1;
                if samples.len() < SAMPLE_LIMIT {
                    let detail = result.reason.unwrap_or_else(|| "failed".to_string());
                    samples.push(format!("{} ({detail})", case.path.display()));
                }
            }
            Outcome::Skipped => {
                skipped += 1;
                if let Some(reason) = result.reason {
                    *skip_reasons.entry(reason).or_default() += 1;
                }
            }
        }

        if (index + 1) % PROGRESS_INTERVAL == 0 {
            let current_pass_rate = passed as f64 / total as f64 * 100.0;
            eprintln!(
                "progress: {}/{} scanned, passed {}, skipped {}, total pass {:.2}%",
                index + 1,
                cases.len(),
                passed,
                skipped,
                current_pass_rate
            );
        }
    }

    let total_pass_rate = if total == 0 {
        0.0
    } else {
        passed as f64 / total as f64 * 100.0
    };
    let executed_pass_rate = if executed == 0 {
        0.0
    } else {
        passed as f64 / executed as f64 * 100.0
    };

    println!("Total cases: {total}");
    println!("Executed: {executed}");
    println!("Passed: {passed}");
    println!("Skipped: {skipped}");
    println!("Total pass rate: {total_pass_rate:.2}%");
    println!("Executed pass rate: {executed_pass_rate:.2}%");
    if !skip_reasons.is_empty() {
        println!("Skip reasons:");
        for (reason, count) in skip_reasons {
            println!("  {reason}: {count}");
        }
    }
    if !samples.is_empty() {
        println!("Sample failures:");
        for sample in samples {
            println!("  {sample}");
        }
    }

    if filter.is_none() && max_cases.is_none() {
        assert!(
            total_pass_rate >= 60.0,
            "expected total pass rate >= 60%, got {total_pass_rate:.2}%"
        );
    }
}
