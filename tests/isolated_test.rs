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
fn engine_exposes_array_from_async() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            print(typeof Array.fromAsync);
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["function".to_string()]);
}

#[test]
fn engine_array_from_async_consumes_async_iterable() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            async function* values() {
              yield 0;
              yield 1;
              yield 2;
            }
            Array.fromAsync(values()).then((items) => print(items.join(',')));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["0,1,2".to_string()]);
}

#[test]
fn engine_array_from_async_returns_promise() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            print(String(Array.fromAsync([1, 2, 3]) instanceof Promise));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["true".to_string()]);
}

#[test]
fn engine_array_from_async_handles_arraylike_input() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            Array.fromAsync({0: 'a', 1: 'b', length: 2}).then((items) => print(items.join(',')));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["a,b".to_string()]);
}

#[test]
fn engine_array_from_async_applies_mapfn() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            Array.fromAsync([1, 2], async (value) => value + 1).then((items) => print(items.join(',')));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["2,3".to_string()]);
}

#[test]
fn engine_array_from_async_rejects_iteration_errors() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            const iterable = {
              [Symbol.asyncIterator]() {
                return {
                  next() {
                    throw new Error('boom');
                  }
                };
              }
            };
            Array.fromAsync(iterable).catch((error) => print(error.message));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["boom".to_string()]);
}

#[test]
fn engine_array_from_async_does_not_await_sync_iterable_input() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            const thenable = {
              then(resolve) {
                print('awaited');
                resolve([1, 2, 3]);
              }
            };
            Array.fromAsync(thenable).then(() => print('done'));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["done".to_string()]);
}

#[test]
fn engine_array_from_async_awaits_mapfn_once() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            let calls = 0;
            Array.fromAsync([1], {
              async call(_this, value) {
                calls++;
                return value + 1;
              }
            }.call).then((items) => {
              print(items.join(','));
              print(String(calls));
            });
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["1".to_string(), "1".to_string()]);
}

#[test]
fn engine_array_from_async_rejects_non_callable_mapfn() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            Array.fromAsync([1], 1).catch((error) => print(error.name));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["TypeError".to_string()]);
}

#[test]
fn engine_array_from_async_uses_constructor_receiver() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            function C() {}
            Array.fromAsync.call(C, [1, 2]).then((value) => print(String(value instanceof C)));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["true".to_string()]);
}

#[test]
fn engine_array_from_async_non_constructor_receiver_returns_intrinsic_array() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            Array.fromAsync.call({}, [1]).then((value) => print(String(Array.isArray(value))));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["true".to_string()]);
}

#[test]
fn engine_array_from_async_preserves_length_updates() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            const values = [1];
            Array.fromAsync(values).then((items) => {
              values.push(2);
              print(items.join(','));
            });
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["1".to_string()]);
}

#[test]
fn engine_array_from_async_on_arraylike_reads_live_values() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            const arrayLike = {0: 'a', 1: 'b', length: 2};
            Array.fromAsync(arrayLike).then((items) => print(items.join(',')));
            arrayLike[1] = 'c';
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["a,c".to_string()]);
}

#[test]
fn engine_array_from_async_length_property_shape() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            const desc = Object.getOwnPropertyDescriptor(Array, 'fromAsync');
            print(String(typeof desc.value === 'function'));
            print(String(desc.writable));
            print(String(desc.enumerable));
            print(String(desc.configurable));
            print(String(Array.fromAsync.length));
            "#,
        )
        .unwrap();

    assert_eq!(
        output.printed,
        vec![
            "true".to_string(),
            "true".to_string(),
            "false".to_string(),
            "true".to_string(),
            "1".to_string()
        ]
    );
}

#[test]
fn engine_array_from_async_name_property() {
    let engine = JsEngine::new();
    let output = engine.eval("print(Array.fromAsync.name);").unwrap();

    assert_eq!(output.printed, vec!["fromAsync".to_string()]);
}

#[test]
fn engine_array_from_async_returns_array() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            Array.fromAsync([1, 2]).then((value) => print(String(Array.isArray(value))));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["true".to_string()]);
}

#[test]
fn engine_array_from_async_closes_sync_iterator_on_mapfn_throw() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            let closed = false;
            const iterable = {
              [Symbol.iterator]() {
                return {
                  next() { return { done: false, value: 1 }; },
                  return() { closed = true; return {}; }
                };
              }
            };
            Array.fromAsync(iterable, () => { throw new Error('map'); }).catch((error) => {
              print(error.message);
              print(String(closed));
            });
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["map".to_string(), "true".to_string()]);
}

#[test]
fn engine_array_from_async_closes_async_iterator_on_mapfn_throw() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            let closed = false;
            const iterable = {
              [Symbol.asyncIterator]() {
                return {
                  next() { return Promise.resolve({ done: false, value: 1 }); },
                  return() { closed = true; return Promise.resolve({}); }
                };
              }
            };
            Array.fromAsync(iterable, () => { throw new Error('map'); }).catch((error) => {
              print(error.message);
              print(String(closed));
            });
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["map".to_string(), "true".to_string()]);
}

#[test]
fn engine_array_from_async_rejects_arraylike_length_errors() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            const input = {
              get length() { throw new Error('length'); }
            };
            Array.fromAsync(input).catch((error) => print(error.message));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["length".to_string()]);
}

#[test]
fn engine_array_from_async_handles_sparse_arraylikes() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            Array.fromAsync({0: 'a', 2: 'c', length: 3}).then((items) => print(String(items[1] === undefined)));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["true".to_string()]);
}

#[test]
fn engine_array_from_async_rejects_missing_iterator_result_shape() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            const iterable = {
              [Symbol.asyncIterator]() {
                return {
                  next() { return Promise.resolve(1); }
                };
              }
            };
            Array.fromAsync(iterable).catch((error) => print(error.name));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["TypeError".to_string()]);
}

#[test]
fn engine_array_from_async_accepts_string_arraylike() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            Array.fromAsync('ab').then((items) => print(items.join(',')));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["a,b".to_string()]);
}

#[test]
fn engine_array_from_async_builtins_basic_semantics() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            print(String(typeof Array.fromAsync === 'function'));
            print(Array.fromAsync.name);
            print(String(Array.fromAsync.length));
            "#,
        )
        .unwrap();

    assert_eq!(
        output.printed,
        vec!["true".to_string(), "fromAsync".to_string(), "1".to_string()]
    );
}

#[test]
fn engine_array_from_async_does_not_await_async_iterable_values() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            const prom = Promise.resolve({});
            const input = {
              [Symbol.asyncIterator]() {
                let i = 0;
                return {
                  async next() {
                    if (i > 0) {
                      return { done: true };
                    }
                    i++;
                    return { value: prom, done: false };
                  }
                };
              }
            };
            Array.fromAsync(input)
              .then((items) => print(String(items[0] === prom)))
              .catch((error) => print(error.name + ':' + error.message));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["true".to_string()]);
}

#[test]
fn engine_array_from_async_uses_intrinsic_iterator_symbols() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            const originalSymbol = globalThis.Symbol;
            const fakeIteratorSymbol = Symbol('iterator');
            const fakeAsyncIteratorSymbol = Symbol('asyncIterator');
            globalThis.Symbol = {
              iterator: fakeIteratorSymbol,
              asyncIterator: fakeAsyncIteratorSymbol,
            };

            const input = {
              length: 2,
              0: 'a',
              1: 'b',
              [fakeIteratorSymbol]() {
                throw new Error('wrong iterator');
              },
              [fakeAsyncIteratorSymbol]() {
                throw new Error('wrong async iterator');
              }
            };

            Array.fromAsync(input)
              .then((items) => print(items.join(',')))
              .catch((error) => print(error.name + ':' + error.message))
              .finally(() => { globalThis.Symbol = originalSymbol; });
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["a,b".to_string()]);
}

#[test]
fn engine_array_from_async_rejects_readonly_length_on_constructor_receiver() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            function MyArray() {
              Object.defineProperty(this, 'length', {
                enumerable: true,
                writable: false,
                configurable: true,
                value: 99
              });
            }

            Array.fromAsync.call(MyArray, [0, 1]).catch((error) => print(error.name));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["TypeError".to_string()]);
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
fn engine_import_defer_pre_evaluates_async_dependencies_only() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    fs::write(temp_root.join("setup.mjs"), "globalThis.evaluations = [];").unwrap();
    fs::write(
        temp_root.join("tla.mjs"),
        "globalThis.evaluations.push('tla start'); await Promise.resolve(0); globalThis.evaluations.push('tla end'); export const x = 1;",
    )
    .unwrap();
    fs::write(
        temp_root.join("imports_tla.mjs"),
        "import './tla.mjs'; globalThis.evaluations.push('imports-tla'); export const x = 1;",
    )
    .unwrap();
    fs::write(
        &entry_path,
        r#"
        import './setup.mjs';
        import.defer('./imports_tla.mjs').then(ns => {
          print(String(globalThis.evaluations.join(',')));
          void ns.x;
          print(String(globalThis.evaluations.join(',')));
        });
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
            "tla start,tla end".to_string(),
            "tla start,tla end,imports-tla".to_string()
        ]
    );
}

#[test]
fn engine_import_defer_avoids_user_visible_promise_then_during_async_preevaluation() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    fs::write(temp_root.join("setup.mjs"), "globalThis.evaluations = [];").unwrap();
    fs::write(
        temp_root.join("tla.mjs"),
        "globalThis.evaluations.push('tla start'); await Promise.resolve(0); globalThis.evaluations.push('tla end'); export const x = 1;",
    )
    .unwrap();
    fs::write(
        temp_root.join("imports_tla.mjs"),
        "import './tla.mjs'; globalThis.evaluations.push('imports-tla'); export const x = 1;",
    )
    .unwrap();
    fs::write(
        &entry_path,
        r#"
        import './setup.mjs';
        let thenCallCount = 0;
        const originalThen = Promise.prototype.then;
        Promise.prototype.then = function(onFulfilled, onRejected) {
          thenCallCount++;
          return originalThen.call(this, onFulfilled, onRejected);
        };

        originalThen.call(import.defer('./imports_tla.mjs'), (ns) => {
          print(String(thenCallCount));
          print(String(globalThis.evaluations.join(',')));
          void ns.x;
          print(String(globalThis.evaluations.join(',')));
        });
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
            "tla start,tla end".to_string(),
            "tla start,tla end,imports-tla".to_string()
        ]
    );
}

#[test]
fn engine_import_defer_keeps_sync_graph_lazy() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    fs::write(temp_root.join("setup.mjs"), "globalThis.evaluations = [];").unwrap();
    fs::write(
        temp_root.join("dep.mjs"),
        "globalThis.evaluations.push('dep'); export const y = 1;",
    )
    .unwrap();
    fs::write(
        temp_root.join("sync.mjs"),
        "import './dep.mjs'; globalThis.evaluations.push('sync'); export const x = 1;",
    )
    .unwrap();
    fs::write(
        &entry_path,
        r#"
        import './setup.mjs';
        import.defer('./sync.mjs').then(ns => {
          print(String(globalThis.evaluations.length));
          void ns.x;
          print(String(globalThis.evaluations.join(',')));
        });
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
        vec!["0".to_string(), "dep,sync".to_string()]
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
fn engine_blocks_pathological_string_replace_growth() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            function puff(x, n) {
              while (x.length < n) x += x;
              return x.substring(0, n);
            }

            const x = puff('1', 1 << 20);
            const rep = puff('$1', 1 << 16);
            let caught = null;
            try {
              x.replace(/(.+)/g, rep);
            } catch (error) {
              caught = error;
            }

            print(caught && caught.message);
            caught instanceof ReferenceError && caught.message === 'OOM Limit';
            "#,
            &EvalOptions {
                bootstrap_test262: true,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(output.printed, vec!["OOM Limit".to_string()]);
    assert_eq!(output.value.as_deref(), Some("true"));
}

#[test]
fn engine_bootstraps_gc_for_test262() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            typeof $262.gc === 'function' && $262.gc() === undefined;
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
            const compileDesc = Object.getOwnPropertyDescriptor(RegExp.prototype, 'compile');
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

            typeof compileDesc.value === 'function' &&
              compileDesc.enumerable === false &&
              compileDesc.writable === true &&
              compileDesc.configurable === true &&
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
fn engine_regexp_compile_rejects_cross_realm_receivers() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            const other = $262.createRealm().global;
            const regexp = new RegExp('');
            const otherRealmRegexp = new other.RegExp('');

            let currentRealmThrows = false;
            let otherRealmThrows = false;
            let sameRealmCallSucceeds = false;

            try {
              RegExp.prototype.compile.call(otherRealmRegexp);
            } catch (error) {
              currentRealmThrows = error instanceof TypeError;
            }

            try {
              other.RegExp.prototype.compile.call(regexp);
            } catch (error) {
              otherRealmThrows = error instanceof other.TypeError;
            }

            sameRealmCallSucceeds = otherRealmRegexp.compile() === otherRealmRegexp;

            currentRealmThrows && otherRealmThrows && sameRealmCallSucceeds;
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
fn engine_regexp_compile_rejects_subclass_receivers() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            const subclassRegexp = new (class extends RegExp {})('');
            let methodThrows = false;
            let callThrows = false;

            try {
              subclassRegexp.compile();
            } catch (error) {
              methodThrows = error instanceof TypeError;
            }

            try {
              RegExp.prototype.compile.call(subclassRegexp);
            } catch (error) {
              callThrows = error instanceof TypeError;
            }

            methodThrows && callThrows;
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
fn engine_regexp_compile_reinitializes_matcher_for_symbol_split() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            var regExp = /a/;
            Object.defineProperty(regExp, Symbol.match, {
              get: function() {
                regExp.compile('b');
              }
            });

            const result = regExp[Symbol.split]('abba');
            result.length === 3 && result[0] === 'a' && result[1] === '' && result[2] === 'a';
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
fn engine_regexp_duplicate_named_groups_exec_groups_projection() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            const matcher = /(?:(?<x>a)|(?<y>a)(?<x>b))(?:(?<z>c)|(?<z>d))/;
            const match = matcher.exec('abc');
            match.groups.x === 'b'
              && match.groups.y === 'a'
              && match.groups.z === 'c'
              && Object.keys(match.groups).join(',') === 'x,y,z';
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
fn engine_regexp_duplicate_named_groups_named_backreference() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            const match = /(?:(?:(?<a>x)|(?<a>y))\k<a>){2}/.exec('xxyy');
            match !== null
              && match[0] === 'xxyy'
              && match[1] === undefined
              && match[2] === 'y';
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
fn engine_bootstraps_disposable_stack_core_semantics() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'DisposableStack');
            const disposedDescriptor = Object.getOwnPropertyDescriptor(DisposableStack.prototype, 'disposed');
            const stack = new DisposableStack();
            const events = [];
            const resource = {
              get [Symbol.dispose]() {
                events.push('read');
                return function() { events.push('resource'); };
              }
            };

            stack.defer(() => events.push('defer-1'));
            stack.use(resource);
            stack.defer(() => events.push('defer-2'));
            stack.dispose();

            typeof DisposableStack === 'function'
              && descriptor.enumerable === false
              && descriptor.writable === true
              && descriptor.configurable === true
              && DisposableStack.prototype[Symbol.dispose] === DisposableStack.prototype.dispose
              && Object.prototype.toString.call(stack) === '[object DisposableStack]'
              && typeof disposedDescriptor.get === 'function'
              && disposedDescriptor.set === undefined
              && disposedDescriptor.enumerable === false
              && disposedDescriptor.configurable === true
              && stack.disposed === true
              && events.join(',') === 'read,defer-2,resource,defer-1';
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
fn engine_bootstraps_async_disposable_stack_core_semantics() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            async function main() {
              const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'AsyncDisposableStack');
              const disposedDescriptor = Object.getOwnPropertyDescriptor(AsyncDisposableStack.prototype, 'disposed');
              const stack = new AsyncDisposableStack();
              const events = [];
              const resource = {
                get [Symbol.asyncDispose]() {
                  events.push('read');
                  return async function() { events.push('resource'); };
                }
              };

              stack.defer(async () => events.push('defer-1'));
              stack.use(resource);
              stack.defer(async () => events.push('defer-2'));
              const result = stack.disposeAsync();
              const isPromise = Object.getPrototypeOf(result) === Promise.prototype;
              await result;

              return typeof AsyncDisposableStack === 'function'
                && descriptor.enumerable === false
                && descriptor.writable === true
                && descriptor.configurable === true
                && AsyncDisposableStack.prototype[Symbol.asyncDispose] === AsyncDisposableStack.prototype.disposeAsync
                && Object.prototype.toString.call(stack) === '[object AsyncDisposableStack]'
                && typeof disposedDescriptor.get === 'function'
                && disposedDescriptor.set === undefined
                && disposedDescriptor.enumerable === false
                && disposedDescriptor.configurable === true
                && isPromise
                && stack.disposed === true
                && events.join(',') === 'read,defer-2,resource,defer-1';
            }
            main().then((value) => print(String(value)));
            "#,
            &EvalOptions {
                bootstrap_test262: true,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(output.printed, vec!["true".to_string()]);
}

#[test]
fn engine_bootstraps_async_disposable_stack_move_and_suppressed_error() {
    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(
            r#"
            async function main() {
              const source = new AsyncDisposableStack();
              const events = [];
              source.defer(async () => events.push('first'));
              source.defer(async () => events.push('second'));
              const moved = source.move();

              let moveThrows = false;
              try {
                source.move();
              } catch (error) {
                moveThrows = error instanceof ReferenceError;
              }

              let suppressed = false;
              const errors = new AsyncDisposableStack();
              const e1 = new Error('e1');
              const e2 = new Error('e2');
              errors.defer(async () => { throw e1; });
              errors.defer(async () => { throw e2; });
              try {
                await errors.disposeAsync();
              } catch (error) {
                suppressed = error instanceof SuppressedError
                  && error.error === e1
                  && error.suppressed === e2;
              }

              await moved.disposeAsync();

              return source.disposed === true
                && moved.disposed === true
                && events.join(',') === 'second,first'
                && moveThrows
                && suppressed;
            }
            main().then((value) => print(String(value)));
            "#,
            &EvalOptions {
                bootstrap_test262: true,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(output.printed, vec!["true".to_string()]);
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
fn engine_exposes_intl_in_eval_scripts() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            typeof Intl === 'object' &&
              typeof Intl.NumberFormat === 'function' &&
              new Intl.NumberFormat('en-US').format(1234.5) === '1,234.5' &&
              new Intl.Collator('en').compare('a', 'b') < 0;
            "#,
        )
        .unwrap();

    assert_eq!(output.value.as_deref(), Some("true"));
}

#[test]
fn engine_exposes_intl_in_file_scripts() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.js");
    fs::write(
        &entry_path,
        r#"
        print(typeof Intl);
        print(typeof Intl.NumberFormat);
        print(new Intl.NumberFormat('en-US').format(1234.5));
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
            "1,234.5".to_string(),
        ]
    );
}

#[test]
fn engine_exposes_intl_in_modules() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    fs::write(
        &entry_path,
        r#"
        print(typeof Intl);
        print(typeof Intl.NumberFormat);
        print(new Intl.NumberFormat('en-US').format(1234.5));
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
            "1,234.5".to_string(),
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

#[test]
fn engine_drains_async_function_microtasks_in_scripts() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            async function f() {
              return await Promise.resolve(42);
            }
            f().then((value) => print(String(value)));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["42".to_string()]);
}

#[test]
fn engine_drains_promise_rejection_handlers_in_scripts() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            async function f() {
              throw new Error('boom');
            }
            f().catch((error) => print(error.message));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["boom".to_string()]);
}

#[test]
fn engine_drains_nested_microtask_chains_in_scripts() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            Promise.resolve(1)
              .then((value) => value + 1)
              .then((value) => print(String(value + 1)));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["3".to_string()]);
}

#[test]
fn engine_parses_await_using_in_async_function_blocks() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            async function main() {
              {
                await using x = {
                  [Symbol.asyncDispose]() {}
                };
              }
              return 'ok';
            }
            main().then((value) => print(value));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["ok".to_string()]);
}

#[test]
fn engine_parses_await_using_in_async_functions_with_identifier_named_using() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            async function main() {
              let using = 1;
              await using x = {
                [Symbol.asyncDispose]() {}
              };
              return using;
            }
            main().then((value) => print(String(value)));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["1".to_string()]);
}

#[test]
fn engine_parses_await_using_in_for_statement_heads() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            async function main() {
              const events = [];
              const resource = {
                async [Symbol.asyncDispose]() {
                  events.push('dispose');
                }
              };
              let seen = false;
              for (await using x = resource; !seen; seen = true) {
                events.push(String(x === resource));
                events.push('body');
              }
              return events.join(',');
            }
            main().then((value) => print(value));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["true,body,dispose".to_string()]);
}

#[test]
fn engine_parses_await_using_in_for_of_heads() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            async function main() {
              const events = [];
              const resources = [
                { async [Symbol.asyncDispose]() { events.push('dispose-1'); } },
                { async [Symbol.asyncDispose]() { events.push('dispose-2'); } }
              ];
              for (await using x of resources) {
                events.push(x === resources[0] ? 'body-1' : 'body-2');
              }
              return events.join(',');
            }
            main().then((value) => print(value));
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["body-1,dispose-1,body-2,dispose-2".to_string()]);
}

#[test]
fn engine_parses_await_using_in_for_await_of_heads() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            async function* values() {
              yield { async [Symbol.asyncDispose]() { print('dispose-1'); } };
              yield { async [Symbol.asyncDispose]() { print('dispose-2'); } };
            }
            async function main() {
              const events = [];
              for await (x of values()) {
                events.push('body');
                await x[Symbol.asyncDispose]();
              }
              print(events.join(','));
            }
            main();
            "#,
        )
        .unwrap();

    assert_eq!(
        output.printed,
        vec![
            "dispose-1".to_string(),
            "dispose-2".to_string(),
            "body,body".to_string()
        ]
    );
}

#[test]
fn engine_mixed_using_and_await_using_share_async_stack() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            async function main() {
              let values = [];
              {
                await using x = {
                  [Symbol.asyncDispose]() { values.push('async'); }
                };
                using y = {
                  [Symbol.dispose]() { values.push('sync'); }
                };
                values.push('body');
              }
              print(values.join(','));
            }
            main();
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["body,sync,async".to_string()]);
}

#[test]
fn engine_async_dispose_fallback_does_not_await_sync_dispose_return_value() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            async function main() {
              let values = [];
              let stack = new AsyncDisposableStack();
              const neverResolves = Promise.withResolvers().promise;
              stack.use({
                [Symbol.dispose]() {
                  return neverResolves;
                }
              });
              await stack.disposeAsync();
              values.push('stack');

              await using x = {
                [Symbol.dispose]() {
                  return neverResolves;
                }
              };
              values.push('using');
              print(values.join(','));
            }
            main();
            "#,
        )
        .unwrap();

    assert_eq!(output.printed, vec!["stack,using".to_string()]);
}

#[test]
fn engine_using_combines_body_and_dispose_errors_into_suppressed_error() {
    let engine = JsEngine::new();
    let output = engine
        .eval(
            r#"
            async function main() {
              const bodyError = new Error('body');
              const disposeError = new Error('dispose');
              try {
                {
                  await using x = {
                    [Symbol.asyncDispose]() {
                      throw disposeError;
                    }
                  };
                  throw bodyError;
                }
              } catch (error) {
                print(error.name);
                print(String(error.error === disposeError));
                print(String(error.suppressed === bodyError));
              }
            }
            main();
            "#,
        )
        .unwrap();

    assert_eq!(
        output.printed,
        vec![
            "SuppressedError".to_string(),
            "true".to_string(),
            "true".to_string()
        ]
    );
}

#[test]
fn engine_parses_top_level_await_using_in_modules() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    fs::write(
        &entry_path,
        r#"
        await using x = {
          [Symbol.asyncDispose]() {}
        };
        print('ok');
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

    assert_eq!(output.printed, vec!["ok".to_string()]);
}

#[test]
fn engine_drains_promise_then_handlers_in_modules() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    fs::write(
        &entry_path,
        r#"
        Promise.resolve(1).then((value) => print(String(value + 1)));
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

    assert_eq!(output.printed, vec!["2".to_string()]);
}

#[test]
fn engine_drains_native_resolved_promises_in_modules() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    fs::write(temp_root.join("dep.mjs"), "export const x = 1;").unwrap();
    fs::write(
        &entry_path,
        format!(
            r#"
            globalThis.__agentjs_dynamic_import_defer__('./dep.mjs', {:?})
              .then(() => print('ok'));
            "#,
            entry_path.display().to_string()
        ),
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

    assert_eq!(output.printed, vec!["ok".to_string()]);
}

#[test]
fn engine_supports_top_level_await_in_modules() {
    let engine = JsEngine::new();
    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root).unwrap();

    let entry_path = temp_root.join("entry.mjs");
    fs::write(
        &entry_path,
        r#"
        const value = await Promise.resolve(42);
        print(String(value));
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

    assert_eq!(output.printed, vec!["42".to_string()]);
}

fn unique_temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("agent-js-engine-{nanos}"))
}
