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
fn engine_supports_dynamic_import_attributes_in_scripts() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.js");
    let json_path = temp_root.join("value.json");
    fs::write(&json_path, "262").unwrap();
    fs::write(
        &entry_path,
        r#"
        import('./value.json', { with: { type: 'json' } })
          .then((module) => print(String(module.default)));
        "#,
    )
    .unwrap();

    let output = engine
        .eval_script_with_options(
            &fs::read_to_string(&entry_path).unwrap(),
            &entry_path,
            &temp_root,
            &Default::default(),
        )
        .unwrap();

    assert_eq!(output.printed, vec!["262".to_string()]);
}

#[test]
fn engine_supports_static_text_module_imports() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    let text_path = temp_root.join("note.txt");
    fs::write(&text_path, "hello from text module").unwrap();
    fs::write(
        &entry_path,
        "import value from './note.txt' with { type: 'text' };\nprint(value);",
    )
    .unwrap();

    let output = engine
        .eval_module_with_options(
            &fs::read_to_string(&entry_path).unwrap(),
            &entry_path,
            &temp_root,
            &Default::default(),
        )
        .unwrap();

    assert_eq!(output.printed, vec!["hello from text module".to_string()]);
}

#[test]
fn engine_supports_static_bytes_module_imports() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    let bytes_path = temp_root.join("value.bin");
    fs::write(&bytes_path, [0_u8, 1, 2, 3]).unwrap();
    fs::write(
        &entry_path,
        r#"
        import value from './value.bin' with { type: 'bytes' };
        print(String(value instanceof Uint8Array));
        print(String(value.buffer.immutable));
        print(String(Array.from(value).join(',')));
        "#,
    )
    .unwrap();

    let output = engine
        .eval_module_with_options(
            &fs::read_to_string(&entry_path).unwrap(),
            &entry_path,
            &temp_root,
            &Default::default(),
        )
        .unwrap();

    assert_eq!(
        output.printed,
        vec![
            "true".to_string(),
            "true".to_string(),
            "0,1,2,3".to_string(),
        ]
    );
}

#[test]
fn engine_rejects_import_source_for_source_text_modules() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.js");
    let module_path = temp_root.join("dep.mjs");
    fs::write(&module_path, "export const value = 1;").unwrap();
    fs::write(
        &entry_path,
        r#"
        import.source('./dep.mjs').catch((error) => print(error.name));
        "#,
    )
    .unwrap();

    let output = engine
        .eval_script_with_options(
            &fs::read_to_string(&entry_path).unwrap(),
            &entry_path,
            &temp_root,
            &Default::default(),
        )
        .unwrap();

    assert_eq!(output.printed, vec!["SyntaxError".to_string()]);
}

#[test]
fn engine_accepts_static_source_phase_import_syntax() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    let module_path = temp_root.join("dep.mjs");
    fs::write(&module_path, "export const value = 1;").unwrap();
    fs::write(&entry_path, "import source source from './dep.mjs';").unwrap();

    let error = engine
        .eval_module_with_options(
            &fs::read_to_string(&entry_path).unwrap(),
            &entry_path,
            &temp_root,
            &Default::default(),
        )
        .unwrap_err();

    assert_eq!(error.name, "SyntaxError");
}

#[test]
fn engine_enforces_immutable_array_buffer_wrappers() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            const immutable = new ArrayBuffer(8).transferToImmutable();
            const view = new DataView(immutable);

            let resizeThrows = false;
            let transferThrows = false;
            let setThrows = false;
            try { immutable.resize(0); } catch (error) { resizeThrows = error instanceof TypeError; }
            try { immutable.transfer(); } catch (error) { transferThrows = error instanceof TypeError; }
            try { view.setUint8(0, 1); } catch (error) { setThrows = error instanceof TypeError; }

            immutable.immutable && resizeThrows && transferThrows && setThrows;
            "#,
        )
        .unwrap();

    assert_eq!(output.value.as_deref(), Some("true"));
}

#[test]
fn engine_bootstraps_abstract_module_source_for_test262() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            const ctor = $262.AbstractModuleSource;
            const prototypeDescriptor = Object.getOwnPropertyDescriptor(ctor, 'prototype');
            const tagDescriptor = Object.getOwnPropertyDescriptor(
              ctor.prototype,
              Symbol.toStringTag
            );

            let throwsTypeError = false;
            try {
              new ctor();
            } catch (error) {
              throwsTypeError = error.constructor === TypeError;
            }

            typeof ctor === 'function' &&
              Object.getPrototypeOf(ctor) === Function.prototype &&
              Object.getPrototypeOf(ctor.prototype) === Object.prototype &&
              prototypeDescriptor.writable === false &&
              prototypeDescriptor.enumerable === false &&
              prototypeDescriptor.configurable === false &&
              tagDescriptor.enumerable === false &&
              tagDescriptor.configurable === true &&
              typeof tagDescriptor.get === 'function' &&
              tagDescriptor.get.call(ctor.prototype) === undefined &&
              throwsTypeError;
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
