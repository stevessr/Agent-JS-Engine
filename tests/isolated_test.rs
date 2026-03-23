use ai_agent::engine::{EvalOptions, JsEngine};
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

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
fn engine_supports_dynamic_import_defer_abrupt_rejects() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.js");
    fs::write(
        &entry_path,
        r#"
        const obj = {
          toString() {
            throw "custom error";
          }
        };

        import.defer(obj).catch((error) => print(String(error)));
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

    assert_eq!(output.printed, vec!["custom error".to_string()]);
}

#[test]
fn engine_supports_static_import_defer_namespace_syntax() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    let module_path = temp_root.join("dep.mjs");
    fs::write(&module_path, "export const value = 41;").unwrap();
    fs::write(
        &entry_path,
        "import defer * as ns from './dep.mjs' with { };\nprint(String(ns.value + 1));",
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

    assert_eq!(output.printed, vec!["42".to_string()]);
}

#[test]
fn engine_defers_module_evaluation_until_property_access() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    fs::write(temp_root.join("setup.mjs"), "globalThis.evaluations = [];").unwrap();
    fs::write(
        temp_root.join("dep_1_1.mjs"),
        "globalThis.evaluations.push(1.1);",
    )
    .unwrap();
    fs::write(
        temp_root.join("dep_1_2.mjs"),
        "globalThis.evaluations.push(1.2); export const foo = 1;",
    )
    .unwrap();
    fs::write(
        temp_root.join("dep_1.mjs"),
        "import './dep_1_1.mjs'; import defer * as ns_1_2 from './dep_1_2.mjs'; globalThis.evaluations.push(1); export { ns_1_2 };",
    )
    .unwrap();
    fs::write(
        &entry_path,
        r#"
        import './setup.mjs';
        import defer * as ns1 from './dep_1.mjs';

        print(String(globalThis.evaluations.length));
        const deferred = ns1.ns_1_2;
        print(String(globalThis.evaluations.join(',')));
        deferred.foo;
        print(String(globalThis.evaluations.join(',')));
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
            "0".to_string(),
            "1.1,1".to_string(),
            "1.1,1,1.2".to_string()
        ]
    );
}

#[test]
fn engine_handles_self_referential_deferred_imports_without_recursing() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    fs::write(
        &entry_path,
        r#"
        import defer * as self from './entry.mjs';

        try {
          self.foo;
        } catch (error) {
          print(error.name);
        }
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

    assert_eq!(output.printed, vec!["TypeError".to_string()]);
}

#[test]
#[ignore = "covered by the real test262 get-other-while-dep-evaluating case; this hand-written repro diverges from the module graph timing"]
fn engine_blocks_deferred_namespace_when_dependency_is_currently_evaluating() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("main.mjs");
    fs::write(
        &entry_path,
        r#"
        import './dep1.mjs';
        print(String(globalThis['evaluating dep2.foo error']?.name));
        print(String(globalThis['evaluating dep2.foo evaluates dep3']));
        print(String(globalThis.dep3evaluated));
        "#,
    )
    .unwrap();
    fs::write(
        temp_root.join("dep1.mjs"),
        r#"
        import defer * as dep2 from './dep2.mjs';
        globalThis.dep3evaluated = false;
        try { dep2.foo; } catch (error) { globalThis['evaluating dep2.foo error'] = error; }
        globalThis['evaluating dep2.foo evaluates dep3'] = globalThis.dep3evaluated;
        "#,
    )
    .unwrap();
    fs::write(
        temp_root.join("dep2.mjs"),
        "import './dep3.mjs'; import './main.mjs';",
    )
    .unwrap();
    fs::write(
        temp_root.join("dep3.mjs"),
        "globalThis.dep3evaluated = true;",
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
            "TypeError".to_string(),
            "false".to_string(),
            "false".to_string()
        ]
    );
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

#[test]
fn engine_rejects_new_import_defer_and_source_syntax() {
    let engine = JsEngine::new();

    for source in [
        "new import.defer('./dep.mjs');",
        "new import.source('./dep.mjs');",
    ] {
        let error = engine.eval(source).unwrap_err();
        assert_eq!(error.name, "SyntaxError");
    }
}

#[test]
fn engine_bootstraps_legacy_regexp_static_accessors() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            const inputDesc = Object.getOwnPropertyDescriptor(RegExp, 'input');
            const aliasInputDesc = Object.getOwnPropertyDescriptor(RegExp, '$_');
            const lastMatchDesc = Object.getOwnPropertyDescriptor(RegExp, 'lastMatch');
            const aliasLastMatchDesc = Object.getOwnPropertyDescriptor(RegExp, '$&');
            const indexDesc = Object.getOwnPropertyDescriptor(RegExp, '$1');

            let getterThrows = false;
            let setterThrows = false;
            class MyRegExp extends RegExp {}
            try { Reflect.get(RegExp, 'input', MyRegExp); } catch (error) { getterThrows = error instanceof TypeError; }
            try { Reflect.set(RegExp, 'input', '', MyRegExp); } catch (error) { setterThrows = error instanceof TypeError; }

            typeof inputDesc.get === 'function' &&
              typeof inputDesc.set === 'function' &&
              typeof aliasInputDesc.get === 'function' &&
              typeof aliasInputDesc.set === 'function' &&
              inputDesc.enumerable === false &&
              inputDesc.configurable === true &&
              lastMatchDesc.set === undefined &&
              aliasLastMatchDesc.set === undefined &&
              typeof lastMatchDesc.get === 'function' &&
              typeof aliasLastMatchDesc.get === 'function' &&
              typeof indexDesc.get === 'function' &&
              indexDesc.set === undefined &&
              getterThrows &&
              setterThrows;
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
fn engine_rejects_invalid_import_call_syntax_variants() {
    let engine = JsEngine::new();

    for source in [
        "let f = () => import();",
        "let f = () => import('./dep.mjs', {}, '');",
        "let f = () => import(...['./dep.mjs']);",
        "let f = () => import.UNKNOWN('./dep.mjs');",
        "let f = () => typeof import;",
        "let f = () => import.source(...['./dep.mjs']);",
        "let f = () => import.source.UNKNOWN('./dep.mjs');",
        "let f = () => typeof import.source;",
        "let f = () => typeof import.source.UNKNOWN;",
        "let f = () => import.defer('./dep.mjs', {});",
    ] {
        let error = engine.eval(source).unwrap_err();
        assert_eq!(error.name, "SyntaxError", "source: {source}");
    }
}

#[test]
fn engine_allows_valid_import_call_trailing_commas() {
    let engine = JsEngine::new();

    for source in [
        "typeof import('./dep.mjs',);",
        "typeof import('./dep.mjs', {},);",
    ] {
        let output = engine.eval(source).unwrap();
        assert_eq!(output.value.as_deref(), Some("object"), "source: {source}");
    }
}

#[test]
fn engine_executes_32_deep_nested_function_calls() {
    run_with_large_stack("engine_executes_32_deep_nested_function_calls", || {
        let engine = JsEngine::new();
        let output = engine
            .eval(
                r#"
                (function(){
                    (function(){
                        (function(){
                            (function(){
                                (function(){
                                    (function(){
                                        (function(){
                                            (function(){
                                                (function(){
                                                    (function(){
                                                        (function(){
                                                            (function(){
                                                                (function(){
                                                                    (function(){
                                                                        (function(){
                                                                            (function(){
                                                                                (function(){
                                                                                    (function(){
                                                                                        (function(){
                                                                                            (function(){
                                                                                                (function(){
                                                                                                    (function(){
                                                                                                        (function(){
                                                                                                            (function(){
                                                                                                                (function(){
                                                                                                                    (function(){
                                                                                                                        (function(){
                                                                                                                            (function(){
                                                                                                                                (function(){
                                                                                                                                    (function(){})()
                                                                                                                                })()
                                                                                                                            })()
                                                                                                                        })()
                                                                                                                    })()
                                                                                                                })()
                                                                                                            })()
                                                                                                        })()
                                                                                                    })()
                                                                                                })()
                                                                                            })()
                                                                                        })()
                                                                                    })()
                                                                                })()
                                                                            })()
                                                                        })()
                                                                    })()
                                                                })()
                                                            })()
                                                        })()
                                                    })()
                                                })()
                                            })()
                                        })()
                                    })()
                                })()
                            })()
                        })()
                    })()
                })();
                'ok';
                "#,
            )
            .unwrap();

        assert_eq!(output.value.as_deref(), Some("ok"));
    });
}

#[test]
fn engine_exposes_temporal_in_eval_scripts() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            typeof Temporal === 'object' &&
              typeof Temporal.Instant.from === 'function' &&
              Temporal.Instant.from('1970-01-01T00:00:00Z').epochNanoseconds === 0n &&
              Temporal.PlainDate.from('2020-01-02').toString() === '2020-01-02';
            "#,
        )
        .unwrap();

    assert_eq!(output.value.as_deref(), Some("true"));
}

#[test]
fn engine_exposes_temporal_in_file_scripts() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.js");
    fs::write(
        &entry_path,
        r#"
        print(typeof Temporal);
        print(typeof Temporal.Instant.from);
        print(String(Temporal.PlainDate.from('2020-01-02')));
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

    assert_eq!(
        output.printed,
        vec![
            "object".to_string(),
            "function".to_string(),
            "2020-01-02".to_string(),
        ]
    );
}

#[test]
fn engine_exposes_temporal_in_modules() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    fs::write(
        &entry_path,
        r#"
        print(typeof Temporal);
        print(typeof Temporal.Instant.from);
        print(String(Temporal.PlainDate.from('2020-01-02')));
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
            "object".to_string(),
            "function".to_string(),
            "2020-01-02".to_string(),
        ]
    );
}

#[test]
fn engine_bootstraps_temporal_in_test262_agents() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            $262.agent.start(`
              $262.agent.report(
                typeof Temporal === 'object' &&
                typeof Temporal.Instant.from === 'function' &&
                String(Temporal.PlainDate.from('2020-01-02')) === '2020-01-02'
                  ? 'ok'
                  : 'bad'
              );
              $262.agent.leaving();
            `);
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

    assert_eq!(output.value.as_deref(), Some("ok"));
}

fn unique_temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("agent-js-engine-{nanos}"))
}
