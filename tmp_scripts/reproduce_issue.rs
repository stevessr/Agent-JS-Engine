use ai_agent::engine::{JsEngine, EvalOptions};
use std::path::PathBuf;
use std::fs;

fn main() {
    let engine = JsEngine::new();
    let current_dir = std::env::current_dir().expect("failed to get current dir");
    let case_path = current_dir.join("test262/test/language/expressions/dynamic-import/import-errored-module.js");
    let case_source = fs::read_to_string(&case_path).expect("failed to read test source");
    
    // Mimic test262_runner::build_source
    let harness_root = current_dir.join("test262/harness");
    let mut source = String::new();
    for include in &["sta.js", "assert.js", "doneprintHandle.js", "asyncHelpers.js"] {
        let contents = fs::read_to_string(harness_root.join(include)).expect("harness missing");
        source.push_str(&contents);
        source.push('\n');
    }
    // Note: import-errored-module.js is NOT a module, so NO globalThis.$DONE = $DONE;
    source.push_str(&case_source);

    let options = EvalOptions {
        bootstrap_test262: true,
        ..Default::default()
    };
    
    // We use eval_script_with_options for non-module tests.
    let result = engine.eval_script_with_options(
        &source,
        &case_path,
        &current_dir.join("test262"), // suite_root
        &options
    ).expect("eval failed");
    
    println!("Value: {:?}", result.value);
    println!("Printed:");
    for line in result.printed {
        println!("  {}", line);
    }
}
