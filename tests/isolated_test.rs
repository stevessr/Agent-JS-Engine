use ai_agent::engine::{EvalOptions, JsEngine};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn engine_executes_basic_javascript() {
    let engine = JsEngine::new();
    let output = engine.eval("const answer = 40 + 2; answer;").unwrap();

    assert_eq!(output.value.as_deref(), Some("42"));
}

#[test]
fn engine_captures_print_output() {
    let engine = JsEngine::new();
    let output = engine.eval("print('hello');").unwrap();

    assert_eq!(output.printed, vec!["hello".to_string()]);
}

#[test]
fn engine_executes_basic_module_imports() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    let dep_path = temp_root.join("dep.mjs");
    fs::write(&dep_path, "export const value = 41;").unwrap();
    fs::write(&entry_path, "import { value } from './dep.mjs'; value + 1;").unwrap();

    let output = engine
        .eval_module_with_options(
            &fs::read_to_string(&entry_path).unwrap(),
            &entry_path,
            &temp_root,
            &Default::default(),
        )
        .unwrap();

    assert_eq!(output.value, None);
}

#[test]
fn engine_bootstraps_create_realm_for_test262() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            const other = $262.createRealm();
            other.evalScript("globalThis.marker = 41;");
            other.global.marker === 41 && other.global.Array !== Array;
            "#,
            &EvalOptions {
                bootstrap_test262: true,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(output.value.as_deref(), Some("true"));
}

#[test]
fn engine_bootstraps_detach_array_buffer_for_test262() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            const buffer = new ArrayBuffer(8);
            $262.detachArrayBuffer(buffer);
            buffer.byteLength;
            "#,
            &EvalOptions {
                bootstrap_test262: true,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(output.value.as_deref(), Some("0"));
}

#[test]
fn engine_bootstraps_test262_agent_broadcasts() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            const sab = new SharedArrayBuffer(Int32Array.BYTES_PER_ELEMENT);
            $262.agent.start(`
              $262.agent.receiveBroadcast(function(sab) {
                const view = new Int32Array(sab);
                Atomics.add(view, 0, 1);
                $262.agent.report(String(Atomics.load(view, 0)));
                $262.agent.leaving();
              });
            `);
            $262.agent.broadcast(sab);
            let report = null;
            while ((report = $262.agent.getReport()) === null) {
              $262.agent.sleep(1);
            }
            report;
            "#,
            &EvalOptions {
                bootstrap_test262: true,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(output.value.as_deref(), Some("1"));
}

fn unique_temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("agent-js-engine-{nanos}"))
}
