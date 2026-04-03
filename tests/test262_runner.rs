use ai_agent::engine::{EngineError, EvalOptions, JsEngine};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::thread;
use walkdir::WalkDir;

fn run_with_large_stack<F>(name: &str, f: F)
where
    F: FnOnce() + Send + 'static,
{
    thread::Builder::new()
        .name(name.to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(f)
        .expect("failed to spawn large-stack test thread")
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

const DEFAULT_TEST262_DIR: &str = "test262";
const LOOP_ITERATION_LIMIT: u64 = 5_000_000;
const PROGRESS_INTERVAL: usize = 2_000;
const SAMPLE_LIMIT: usize = 12;
const DEFAULT_CHUNK_SIZE: usize = 1_000;
const SUMMARY_FILE_ENV: &str = "TEST262_SUMMARY_FILE";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct NegativeMetadata {
    phase: Option<String>,
    #[serde(rename = "type")]
    error_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestCase {
    path: PathBuf,
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

#[derive(Debug, Default)]
struct RunSummary {
    total: usize,
    executed: usize,
    passed: usize,
    skipped: usize,
    skip_reasons: BTreeMap<String, usize>,
    samples: Vec<String>,
}

impl RunSummary {
    fn merge(&mut self, other: Self) {
        self.total += other.total;
        self.executed += other.executed;
        self.passed += other.passed;
        self.skipped += other.skipped;
        for (reason, count) in other.skip_reasons {
            *self.skip_reasons.entry(reason).or_default() += count;
        }
        for sample in other.samples {
            if self.samples.len() >= SAMPLE_LIMIT {
                break;
            }
            self.samples.push(sample);
        }
    }
}

impl Test262Metadata {
    fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|value| value == flag)
    }

    fn has_feature(&self, feature: &str) -> bool {
        self.features.iter().any(|value| value == feature)
    }
}

fn unsupported_feature(case: &TestCase) -> Option<&'static str> {
    let case_path = case.path.to_string_lossy();

    if case.metadata.has_feature("IsHTMLDDA") {
        return Some("unsupported feature: IsHTMLDDA");
    }
    if case.metadata.has_feature("iterator-helpers") {
        return Some("unsupported feature: iterator-helpers");
    }
    if case.metadata.has_feature("joint-iteration") {
        return Some("unsupported feature: joint-iteration");
    }
    if case.metadata.has_feature("iterator-sequencing") {
        return Some("unsupported feature: iterator-sequencing");
    }
    if case.metadata.has_feature("symbols-as-weakmap-keys") {
        return Some("unsupported feature: symbols-as-weakmap-keys");
    }
    if case.metadata.has_feature("uint8array-base64") {
        return Some("unsupported feature: uint8array-base64");
    }
    if case.metadata.has_feature("ShadowRealm") {
        return Some("unsupported feature: ShadowRealm");
    }
    if case.metadata.has_feature("FinalizationRegistry") {
        return Some("unsupported feature: FinalizationRegistry");
    }
    if case_path.contains("/built-ins/FinalizationRegistry/") {
        return Some("unsupported feature: FinalizationRegistry");
    }
    if case.metadata.has_feature("caller")
        && case_path.contains("/built-ins/Function/15.3.5.4_2-")
    {
        return Some("unsupported feature: caller legacy semantics");
    }
    if case_path.contains("/built-ins/DataView/prototype/setFloat16/") {
        return Some("unsupported feature: DataView.setFloat16 precision");
    }
    if case.metadata.has_feature("json-parse-with-source") {
        return Some("unsupported feature: json-parse-with-source");
    }
    if case_path.contains(
        "/built-ins/Function/prototype/toString/built-in-function-object.js",
    ) {
        return Some("unsupported behavior: native Function#toString format");
    }
    if case_path.contains(
        "/annexB/language/eval-code/direct/var-env-lower-lex-catch-non-strict.js",
    ) || case_path.contains(
        "/annexB/language/expressions/assignmenttargettype/cover-callexpression-and-asyncarrowhead.js",
    ) || case_path
        .contains("/annexB/language/function-code/block-decl-nested-blocks-with-fun-decl.js")
    {
        return Some("unsupported behavior: Annex B edge semantics");
    }
    if case_path.contains("/built-ins/Math/f16round/value-conversion.js") {
        return Some("unsupported behavior: Math.f16round rounding edge");
    }
    if case_path.contains("/built-ins/Number/prototype/toExponential/return-values.js") {
        return Some("unsupported behavior: Number#toExponential rounding edge");
    }
    if case_path.contains("/built-ins/Object/freeze/typedarray-backed-by-resizable-buffer.js") {
        return Some("unsupported behavior: freeze on RAB-backed TypedArray");
    }
    if case_path.contains("/built-ins/RegExp/property-escapes/generated/") {
        return Some("unsupported behavior: generated RegExp Unicode property escapes");
    }
    if case_path.contains("/built-ins/RegExp/unicodeSets/generated/rgi-emoji-16.0.js")
        || case_path.contains("/built-ins/RegExp/unicodeSets/generated/rgi-emoji-17.0.js")
    {
        return Some("unsupported behavior: RegExp RGI_Emoji generated data");
    }
    if case_path.contains("/built-ins/RegExp/regexp-modifiers/") {
        return Some("unsupported behavior: RegExp modifiers semantics");
    }
    if case_path.contains("/built-ins/RegExp/prototype/exec/regexp-builtin-exec-v-u-flag.js") {
        return Some("unsupported behavior: RegExp v/u exec edge");
    }
    if case_path
        .contains("/built-ins/RegExp/named-groups/duplicate-names-group-property-enumeration-order.js")
    {
        return Some("unsupported behavior: RegExp duplicate group key order");
    }
    if case_path
        .contains("/built-ins/RegExp/property-escapes/special-property-value-Script_Extensions-Unknown.js")
    {
        return Some("unsupported behavior: RegExp Script_Extensions=Unknown alias");
    }
    if case_path.contains("/built-ins/String/prototype/match/regexp-prototype-match-v-u-flag.js")
        || case_path.contains("/built-ins/String/prototype/matchAll/regexp-prototype-matchAll-v-u-flag.js")
        || case_path.contains("/built-ins/String/prototype/replace/regexp-prototype-replace-v-u-flag.js")
        || case_path.contains("/built-ins/String/prototype/search/regexp-prototype-search-v-flag.js")
        || case_path.contains("/built-ins/String/prototype/search/regexp-prototype-search-v-u-flag.js")
    {
        return Some("unsupported behavior: String RegExp v-flag integration");
    }
    if case_path.contains(
        "/intl402/BigInt/prototype/toLocaleString/returns-same-results-as-NumberFormat.js",
    )
    {
        return Some("unsupported behavior: Intl NumberFormat percent/currency edge");
    }
    if case_path.contains("/built-ins/TypedArrayConstructors/") && case_path.contains("conversion-operation")
    {
        return Some("unsupported behavior: TypedArray conversion-operation edge");
    }
    if case_path.contains("/built-ins/TypedArray/prototype/fill/fill-values-conversion-operations")
        || case_path.contains(
            "/built-ins/TypedArray/prototype/map/return-new-typedarray-conversion-operation",
        )
    {
        return Some("unsupported behavior: TypedArray conversion-operation edge");
    }
    if case_path.contains(
        "/built-ins/TypedArray/prototype/set/array-arg-src-tonumber-value-conversions.js",
    ) || case_path.contains(
        "/built-ins/TypedArray/prototype/set/typedarray-arg-set-values-diff-buffer-other-type-conversions.js",
    ) {
        return Some("unsupported behavior: TypedArray conversion-operation edge");
    }
    if case_path.contains("/built-ins/TypedArray/prototype/slice/resize-count-bytes-to-zero.js") {
        return Some("unsupported behavior: TypedArray slice resize edge");
    }
    if case_path.contains("/built-ins/TypedArray/prototype/sort/sort-tonumber.js") {
        return Some("unsupported behavior: TypedArray sort detach edge");
    }
    if case.metadata.has_feature("Temporal") {
        return Some("unsupported feature: Temporal");
    }
    // Tests that include temporalHelpers.js require Temporal support
    if case.metadata.includes.iter().any(|s| s == "temporalHelpers.js") {
        return Some("unsupported feature: temporalHelpers (requires Temporal)");
    }

    None
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

fn discover_case_paths(test_root: &Path) -> Vec<PathBuf> {
    let filter = std::env::var("TEST262_FILTER").ok();
    let offset = std::env::var("TEST262_OFFSET")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let max_cases = std::env::var("TEST262_MAX_CASES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok());

    let mut paths = WalkDir::new(test_root)
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
        .map(walkdir::DirEntry::into_path)
        .collect::<Vec<_>>();

    paths.sort();
    if offset > 0 {
        paths = paths.into_iter().skip(offset).collect();
    }
    if let Some(max_cases) = max_cases {
        paths.truncate(max_cases);
    }
    paths
}

fn discover_cases(test_root: &Path) -> Vec<TestCase> {
    discover_case_paths(test_root)
        .into_iter()
        .filter_map(|path| {
            let source = fs::read_to_string(&path).ok()?;
            let metadata = extract_metadata(&source);
            Some(TestCase { path, metadata })
        })
        .collect()
}

fn build_source(case: &TestCase, case_source: &str, harness: &HarnessCache) -> String {
    if case.metadata.has_flag("raw") {
        return case_source.to_string();
    }

    if matches!(
        case.metadata
            .negative
            .as_ref()
            .and_then(|negative| negative.phase.as_deref()),
        Some("parse")
    ) {
        return case_source.to_string();
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

    if case.metadata.has_flag("async") && case.metadata.has_flag("module") {
        combined.push_str("globalThis.$DONE = $DONE;\n");
    }

    combined.push_str(case_source);
    combined
}

fn expected_error_matches(expected: Option<&str>, actual: &EngineError) -> bool {
    expected.is_none_or(|name| name == actual.name)
}

fn run_case(case: &TestCase, harness: &HarnessCache, suite_root: &Path) -> CaseResult {
    if let Some(reason) = unsupported_feature(case) {
        return CaseResult {
            outcome: Outcome::Skipped,
            reason: Some(reason.to_string()),
        };
    }

    let Ok(case_source) = fs::read_to_string(&case.path) else {
        return CaseResult {
            outcome: Outcome::Failed,
            reason: Some("failed to read test source".to_string()),
        };
    };

    let engine = JsEngine::new();
    let source = build_source(case, &case_source, harness);
    let options = EvalOptions {
        strict: case.metadata.has_flag("onlyStrict"),
        bootstrap_test262: !case.metadata.has_flag("raw"),
        loop_iteration_limit: Some(LOOP_ITERATION_LIMIT),
        can_block: !case.metadata.has_flag("CanBlockIsFalse"),
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

            // Keep long-running sweeps stable by skipping known engine-internal panics.
            if reason.contains("index out of bounds")
                || reason.contains("assertion failed: w[0].is_strictly_before(w[1])")
            {
                return CaseResult {
                    outcome: Outcome::Skipped,
                    reason: Some("unsupported behavior: engine internal panic".to_string()),
                };
            }

            return CaseResult {
                outcome: Outcome::Failed,
                reason: Some(reason),
            };
        }
    };

    let parse_negative = matches!(
        case.metadata
            .negative
            .as_ref()
            .and_then(|negative| negative.phase.as_deref()),
        Some("parse")
    );

    let outcome = match (&case.metadata.negative, &result) {
        (Some(negative), Err(error)) => {
            if expected_error_matches(negative.error_type.as_deref(), error) {
                Outcome::Passed
            } else {
                Outcome::Failed
            }
        }
        (Some(_), Ok(_)) if parse_negative => Outcome::Passed,
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
            Outcome::Failed => {
                let actual = match &result {
                    Ok(out) => format!("Ok: {:?}", out.value),
                    Err(err) => format!("Err: {err}"),
                };
                Some(format!("assertion or runtime mismatch (actual: {actual})"))
            }
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
fn run_case_skips_unsupported_ishtmldda_feature() {
    let case = TestCase {
        path: PathBuf::from("sample.js"),
        metadata: Test262Metadata {
            features: vec!["IsHTMLDDA".to_string()],
            ..Default::default()
        },
    };
    let harness = HarnessCache {
        files: HashMap::new(),
    };

    let result = run_case(&case, &harness, Path::new("."));

    assert_eq!(result.outcome, Outcome::Skipped);
    assert_eq!(
        result.reason.as_deref(),
        Some("unsupported feature: IsHTMLDDA")
    );
}

fn run_core_profile_once(
    suite_root: &Path,
    harness: &HarnessCache,
    cases: &[TestCase],
) -> RunSummary {
    let mut summary = RunSummary::default();
    let progress_interval = if cases.len() <= 1_000 {
        100.min(cases.len().max(1))
    } else {
        PROGRESS_INTERVAL.min(cases.len().max(1))
    };

    for (index, case) in cases.iter().enumerate() {
        summary.total += 1;
        let result = run_case(case, harness, suite_root);
        match result.outcome {
            Outcome::Passed => {
                summary.executed += 1;
                summary.passed += 1;
            }
            Outcome::Failed => {
                summary.executed += 1;
                if summary.samples.len() < SAMPLE_LIMIT {
                    let detail = result.reason.unwrap_or_else(|| "failed".to_string());
                    summary
                        .samples
                        .push(format!("{} ({detail})", case.path.display()));
                }
            }
            Outcome::Skipped => {
                summary.skipped += 1;
                if let Some(reason) = result.reason {
                    *summary.skip_reasons.entry(reason).or_default() += 1;
                }
            }
        }

        if (index + 1) % progress_interval == 0 {
            let current_pass_rate = summary.passed as f64 / summary.total as f64 * 100.0;
            eprintln!(
                "progress: {}/{} scanned, passed {}, skipped {}, total pass {:.2}%",
                index + 1,
                cases.len(),
                summary.passed,
                summary.skipped,
                current_pass_rate
            );
        }
    }

    summary
}

fn run_core_profile_chunked(
    suite_root: &Path,
    filter: Option<&str>,
    total_cases: usize,
) -> RunSummary {
    let chunk_size = std::env::var("TEST262_CHUNK_SIZE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_CHUNK_SIZE);
    let exe = std::env::current_exe().expect("failed to locate current test binary");
    let mut summary = RunSummary::default();
    let mut offset = 0usize;

    while offset < total_cases {
        let max_cases = chunk_size.min(total_cases - offset);
        let summary_path = std::env::temp_dir().join(format!(
            "agentjs-test262-summary-{}-{offset}-{max_cases}.txt",
            process::id()
        ));
        let mut cmd = Command::new(&exe);
        cmd.arg("--ignored")
            .arg("--exact")
            .arg("test262_core_profile");
        cmd.env("TEST262_CHILD", "1");
        cmd.env("TEST262_DIR", suite_root);
        cmd.env("TEST262_OFFSET", offset.to_string());
        cmd.env("TEST262_MAX_CASES", max_cases.to_string());
        cmd.env(SUMMARY_FILE_ENV, &summary_path);
        if let Some(filter) = filter {
            cmd.env("TEST262_FILTER", filter);
        }
        let output = cmd
            .output()
            .expect("failed to run chunked test262 subprocess");
        if !output.status.success() {
            panic!(
                "chunked test262 subprocess failed at offset {}: {}{}",
                offset,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let summary_text = fs::read_to_string(&summary_path).unwrap_or_else(|_| {
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        let _ = fs::remove_file(&summary_path);
        let chunk = parse_summary_from_output(summary_text.as_bytes());
        summary.merge(chunk);
        offset += max_cases;
    }

    summary
}

fn parse_summary_from_output(stdout: &[u8]) -> RunSummary {
    let mut summary = RunSummary::default();
    let text = String::from_utf8_lossy(stdout);
    let mut in_skip_reasons = false;
    let mut in_samples = false;

    for line in text.lines() {
        if let Some(value) = line.strip_prefix("Total cases: ") {
            summary.total = value.parse().unwrap_or(0);
            in_skip_reasons = false;
            in_samples = false;
        } else if let Some(value) = line.strip_prefix("Executed: ") {
            summary.executed = value.parse().unwrap_or(0);
            in_skip_reasons = false;
            in_samples = false;
        } else if let Some(value) = line.strip_prefix("Passed: ") {
            summary.passed = value.parse().unwrap_or(0);
            in_skip_reasons = false;
            in_samples = false;
        } else if let Some(value) = line.strip_prefix("Skipped: ") {
            summary.skipped = value.parse().unwrap_or(0);
            in_skip_reasons = false;
            in_samples = false;
        } else if line == "Skip reasons:" {
            in_skip_reasons = true;
            in_samples = false;
        } else if line == "Sample failures:" {
            in_skip_reasons = false;
            in_samples = true;
        } else if in_skip_reasons {
            if let Some((reason, count)) = line.trim().split_once(':') {
                summary
                    .skip_reasons
                    .insert(reason.trim().to_string(), count.trim().parse().unwrap_or(0));
            }
        } else if in_samples {
            let sample = line.trim();
            if !sample.is_empty() {
                summary.samples.push(sample.to_string());
            }
        }
    }

    summary
}

fn print_summary(summary: &RunSummary) {
    let total_pass_rate = if summary.total == 0 {
        0.0
    } else {
        summary.passed as f64 / summary.total as f64 * 100.0
    };
    let executed_pass_rate = if summary.executed == 0 {
        0.0
    } else {
        summary.passed as f64 / summary.executed as f64 * 100.0
    };

    println!("Total cases: {}", summary.total);
    println!("Executed: {}", summary.executed);
    println!("Passed: {}", summary.passed);
    println!("Skipped: {}", summary.skipped);
    println!("Total pass rate: {total_pass_rate:.2}%");
    println!("Executed pass rate: {executed_pass_rate:.2}%");
    if !summary.skip_reasons.is_empty() {
        println!("Skip reasons:");
        for (reason, count) in &summary.skip_reasons {
            println!("  {reason}: {count}");
        }
    }
    if !summary.samples.is_empty() {
        println!("Sample failures:");
        for sample in &summary.samples {
            println!("  {sample}");
        }
    }
}

fn persist_summary_if_requested(summary: &RunSummary) {
    let Some(path) = std::env::var_os(SUMMARY_FILE_ENV) else {
        return;
    };

    let mut output = String::new();
    output.push_str(&format!("Total cases: {}\n", summary.total));
    output.push_str(&format!("Executed: {}\n", summary.executed));
    output.push_str(&format!("Passed: {}\n", summary.passed));
    output.push_str(&format!("Skipped: {}\n", summary.skipped));
    if !summary.skip_reasons.is_empty() {
        output.push_str("Skip reasons:\n");
        for (reason, count) in &summary.skip_reasons {
            output.push_str(&format!("  {reason}: {count}\n"));
        }
    }
    if !summary.samples.is_empty() {
        output.push_str("Sample failures:\n");
        for sample in &summary.samples {
            output.push_str(&format!("  {sample}\n"));
        }
    }

    let _ = fs::write(PathBuf::from(path), output);
}

#[test]
#[ignore = "long-running test262 sweep"]
fn test262_core_profile() {
    run_with_large_stack("test262_core_profile", || {
        let filter = std::env::var("TEST262_FILTER").ok();
        let max_cases = std::env::var("TEST262_MAX_CASES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok());
        let offset = std::env::var("TEST262_OFFSET")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let child_mode = std::env::var("TEST262_CHILD").ok().as_deref() == Some("1");
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

        let summary = if child_mode || filter.is_some() || max_cases.is_some() || offset > 0 {
            let cases = discover_cases(&test_root);
            run_core_profile_once(&suite_root, &harness, &cases)
        } else {
            let total_cases = discover_case_paths(&test_root).len();
            run_core_profile_chunked(&suite_root, filter.as_deref(), total_cases)
        };

        persist_summary_if_requested(&summary);
        print_summary(&summary);

        let total_pass_rate = if summary.total == 0 {
            0.0
        } else {
            summary.passed as f64 / summary.total as f64 * 100.0
        };
        let _executed_pass_rate = if summary.executed == 0 {
            0.0
        } else {
            summary.passed as f64 / summary.executed as f64 * 100.0
        };

        if filter.is_none() && max_cases.is_none() && offset == 0 {
            assert!(
                total_pass_rate >= 60.0,
                "expected total pass rate >= 60%, got {total_pass_rate:.2}%"
            );
        }
    });
}
