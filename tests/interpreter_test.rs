use ai_agent::engine::interpreter::RuntimeError;
use ai_agent::engine::{Interpreter, JsValue};
use ai_agent::lexer::Lexer;
use ai_agent::parser::Parser;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn eval_with_interpreter(source: &str) -> JsValue {
    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");
    let mut interpreter = Interpreter::new();
    interpreter
        .eval_program(&program)
        .expect("program should evaluate")
}

fn eval_with_interpreter_result(source: &str) -> Result<JsValue, RuntimeError> {
    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");
    let mut interpreter = Interpreter::new();
    interpreter.eval_program(&program)
}

fn write_temp_modules(files: &[(&str, &str)]) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("agent_js_engine_modules_{unique}"));
    fs::create_dir_all(&root).expect("temp module dir should be created");
    for (relative_path, source) in files {
        let file_path = root.join(relative_path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).expect("module parent dir should be created");
        }
        fs::write(&file_path, source).expect("module source should be written");
    }
    root
}

#[test]
fn interpreter_executes_function_declaration_calls() {
    let result = eval_with_interpreter(
        r#"
        function add(a, b) {
            return a + b;
        }

        add(20, 22);
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_executes_function_declaration_without_parameters() {
    let result = eval_with_interpreter(
        r#"
        function answer() {
            return 42;
        }

        answer();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_executes_single_parameter_function_calls() {
    let result = eval_with_interpreter(
        r#"
        function inc(value) {
            return value + 1;
        }

        inc(41);
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_binds_multiple_function_parameters() {
    let result = eval_with_interpreter(
        r#"
        function second(a, b) {
            return b;
        }

        second(1, 42);
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_destructuring_function_parameters() {
    let result = eval_with_interpreter(
        r#"
        function total({ base } = { base: 20 }, [extra, ...rest]) {
            return base + extra + rest[0];
        }

        total(undefined, [10, 12]);
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_executes_function_expressions() {
    let result = eval_with_interpreter(
        r#"
        let twice = function(value) {
            return value * 2;
        };

        twice(21);
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_executes_async_function_declaration_syntax() {
    let result = eval_with_interpreter(
        r#"
        async function addOne(value) {
            return value + 1;
        }

        await addOne(41);
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_executes_async_arrow_function_syntax() {
    let result = eval_with_interpreter(
        r#"
        let addOne = async value => value + 1;
        await addOne(41);
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_uses_lexical_this_for_arrow_functions() {
    let result = eval_with_interpreter(
        r#"
        let obj = {
            value: 42,
            getArrow() { return () => this.value; }
        };
        let fnRef = obj.getArrow();
        let other = { value: 1, fnRef };
        other.fnRef();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_rejects_new_on_arrow_functions() {
    let error = eval_with_interpreter_result(
        r#"
        let fnRef = () => 42;
        new fnRef();
        "#,
    )
    .expect_err("arrow functions should not be constructible");

    assert!(matches!(error, RuntimeError::TypeError(_)));
    assert!(format!("{error}").contains("not a constructor"));
}

#[test]
fn interpreter_arrow_functions_do_not_expose_prototype() {
    let result = eval_with_interpreter(
        r#"
        let fnRef = () => 42;
        fnRef.prototype;
        "#,
    );

    assert_eq!(result, JsValue::Undefined);
}

#[test]
fn interpreter_supports_promise_resolve_and_await() {
    let result = eval_with_interpreter("await Promise.resolve(42);");

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_promise_then_chains() {
    let result = eval_with_interpreter(
        r#"
        await Promise.resolve(20)
            .then(value => value + 1)
            .then(value => value * 2);
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_pending_promise_then_chains() {
    let result = eval_with_interpreter(
        r#"
        let resolveLater;
        let promise = new Promise(resolve => {
            resolveLater = resolve;
        });
        let chained = promise.then(value => value + 1);
        resolveLater(41);
        await chained;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_adopts_promises_returned_from_then_handlers() {
    let result = eval_with_interpreter(
        r#"
        await Promise.resolve(41).then(value => Promise.resolve(value + 1));
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_promise_catch_chains() {
    let result = eval_with_interpreter(
        r#"
        await Promise.reject(41).catch(value => value + 1);
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_pending_promise_catch_chains() {
    let result = eval_with_interpreter(
        r#"
        let rejectLater;
        let promise = new Promise((resolve, reject) => {
            rejectLater = reject;
        });
        let chained = promise.catch(value => value + 1);
        rejectLater(41);
        await chained;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_promise_finally() {
    let result = eval_with_interpreter(
        r#"
        let total = 40;
        await Promise.resolve(2).finally(() => { total = total + 1; });
        total + 1;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_pending_promise_finally() {
    let result = eval_with_interpreter(
        r#"
        let total = 1;
        let resolveLater;
        let promise = new Promise(resolve => {
            resolveLater = resolve;
        });
        let chained = promise.finally(() => {
            total = total + 1;
        });
        resolveLater(40);
        let value = await chained;
        total + value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_waits_for_promises_returned_from_finally_handlers() {
    let result = eval_with_interpreter(
        r#"
        let start;
        let release;
        let value = new Promise(resolve => {
            start = resolve;
        }).finally(() => {
            return new Promise(resolve => {
                release = resolve;
            });
        });
        start(41);
        await Promise.resolve(0);
        release(0);
        await value + 1;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_new_promise_executor() {
    let result = eval_with_interpreter(
        r#"
        await new Promise((resolve, reject) => {
            resolve(42);
        });
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_generator_function_iteration() {
    let result = eval_with_interpreter(
        r#"
        function* numbers() {
            yield 10;
            yield 12;
            return 20;
        }

        let iterator = numbers();
        let first = iterator.next();
        let second = iterator.next();
        let third = iterator.next();
        first.value + second.value + third.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_yield_delegate_iteration() {
    let result = eval_with_interpreter(
        r#"
        function* inner() {
            yield 10;
            return 12;
        }

        function* outer() {
            let finalValue = yield* inner();
            return finalValue + 20;
        }

        let iterator = outer();
        let first = iterator.next();
        let second = iterator.next();
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_generator_return_method() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            yield 1;
            yield 2;
        }

        let iterator = values();
        let first = iterator.next();
        let closed = iterator.return(41);
        first.value + closed.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_passes_values_back_into_generator_next_calls() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let input = yield 1;
            return input + 42;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next(0);
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(43.0));
}

#[test]
fn interpreter_forwards_next_values_through_yield_delegate() {
    let result = eval_with_interpreter(
        r#"
        function* inner() {
            let input = yield 1;
            return input + 42;
        }

        function* outer() {
            return yield* inner();
        }

        let iterator = outer();
        let first = iterator.next();
        let second = iterator.next(0);
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(43.0));
}

#[test]
fn interpreter_supports_generator_throw_method() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            yield 1;
        }

        let iterator = values();
        iterator.next();
        try {
            iterator.throw(42);
        } catch (error) {
            error;
        }
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_throws_into_suspended_generators_and_hits_catch_blocks() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            try {
                yield 1;
                return 0;
            } catch (error) {
                return error + 42;
            }
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.throw(0);
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(43.0));
}

#[test]
fn interpreter_runs_generator_finally_blocks_during_return() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            try {
                yield 1;
            } finally {
                yield 2;
            }
            return 40;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.return(40);
        let third = iterator.next();
        first.value + second.value + third.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(43.0));
}

#[test]
fn interpreter_supports_async_generator_method_iteration() {
    let result = eval_with_interpreter(
        r#"
        let source = {
            async *items() {
                yield 19;
                return 23;
            }
        };

        let iterator = source.items();
        let first = await iterator.next();
        let second = await iterator.next();
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_async_generator_function_iteration() {
    let result = eval_with_interpreter(
        r#"
        async function* values() {
            yield 19;
            return 23;
        }

        let iterator = values();
        let first = await iterator.next();
        let second = await iterator.next();
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_awaits_promises_yielded_from_async_generators() {
    let result = eval_with_interpreter(
        r#"
        async function* values() {
            yield Promise.resolve(19);
            return Promise.resolve(23);
        }

        let iterator = values();
        let first = await iterator.next();
        let second = await iterator.next();
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_for_await_over_async_generators() {
    let result = eval_with_interpreter(
        r#"
        async function* values() {
            yield 19;
            yield 23;
        }

        let total = 0;
        for await (const value of values()) {
            total = total + value;
        }
        total;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_rejects_async_generator_next_promises_on_errors() {
    let result = eval_with_interpreter(
        r#"
        async function* values() {
            throw 42;
        }

        let iterator = values();
        try {
            await iterator.next();
        } catch (error) {
            error;
        }
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_async_generator_throw_promises() {
    let result = eval_with_interpreter(
        r#"
        async function* values() {
            try {
                yield 1;
            } catch (error) {
                return error + 42;
            }
        }

        let iterator = values();
        let first = await iterator.next();
        let second = await iterator.throw(0);
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(43.0));
}

#[test]
fn interpreter_supports_for_await_of_statement() {
    let result = eval_with_interpreter(
        r#"
        let total = 0;
        for await (const value of [19, 23]) {
            total = total + value;
        }
        total;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_for_of_over_generator_iterators() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            yield 19;
            yield 23;
        }

        let total = 0;
        for (const value of values()) {
            total = total + value;
        }
        total;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_generator_yields_inside_while_loops() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let i = 0;
            while (i < 2) {
                yield 19 + i * 4;
                i = i + 1;
            }
            return 0;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next();
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_generator_yields_inside_for_loops() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            for (let i = 0; i < 3; i = i + 1) {
                if (i === 1) {
                    continue;
                }
                yield 20 + i;
            }
            return 0;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next();
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_generator_yields_inside_for_of_loops() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            for (const value of [19, 23]) {
                yield value;
            }
            return 0;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next();
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_generator_yields_in_call_arguments() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let add = (left, right) => left + right;
            return add(yield 20, 22);
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next(20);
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(62.0));
}

#[test]
fn interpreter_supports_generator_yields_in_member_expressions() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let object = { answer: 42 };
            return object[yield "answer"];
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next("answer");
        first.value.length + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(48.0));
}

#[test]
fn interpreter_supports_generator_yields_in_member_assignment_targets() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let object = {};
            object[yield "answer"] = 42;
            return object.answer;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next("answer");
        first.value.length + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(48.0));
}

#[test]
fn interpreter_supports_generator_yields_in_array_and_object_literals() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let array = [yield 19, 23];
            let object = { total: array[0] + array[1] };
            return object.total;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next(19);
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(61.0));
}

#[test]
fn interpreter_supports_generator_yields_in_destructuring_defaults() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let [value = yield 19] = [];
            return value + 23;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next(19);
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(61.0));
}

#[test]
fn interpreter_supports_generator_yields_in_destructuring_assignments() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let value = 0;
            [value = yield 19] = [];
            return value + 23;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next(19);
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(61.0));
}

#[test]
fn interpreter_supports_generator_yields_in_destructuring_computed_keys() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let value = 0;
            ({ [yield "answer"]: value } = { answer: 42 });
            return value;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next("answer");
        first.value.length + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(48.0));
}

#[test]
fn interpreter_supports_generator_yields_in_for_of_destructuring_bindings() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            for (const [value = yield 19] of [[]]) {
                return value + 23;
            }
            return 0;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next(19);
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(61.0));
}

#[test]
fn interpreter_supports_generator_yields_in_catch_binding_patterns() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            try {
                throw { answer: 42 };
            } catch ({ [yield "answer"]: value }) {
                return value;
            }
            return 0;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next("answer");
        first.value.length + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(48.0));
}

#[test]
fn interpreter_supports_generator_yields_in_template_literals() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            return `value:${yield 42}`;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next("");
        first.value + second.value.length;
        "#,
    );

    assert_eq!(result, JsValue::Number(48.0));
}

#[test]
fn interpreter_supports_generator_yields_in_update_expressions() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let object = { answer: 41 };
            return ++object[yield "answer"];
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next("answer");
        first.value.length + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(48.0));
}

#[test]
fn interpreter_supports_generator_yields_in_delete_expressions() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let object = { answer: 42 };
            return delete object[yield "answer"];
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next("answer");
        first.value.length + (second.value ? 42 : 0);
        "#,
    );

    assert_eq!(result, JsValue::Number(48.0));
}

#[test]
fn interpreter_supports_generator_yields_in_tagged_template_expressions() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let object = {
                base: 20,
                tag(strings, value) {
                    return this.base + value;
                }
            };
            return object.tag`${yield 22}`;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next(22);
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(64.0));
}

#[test]
fn interpreter_supports_generator_yields_in_switch_statements() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            switch (2) {
                case yield 1:
                    return 0;
                case yield 2:
                    yield 39;
                    break;
                default:
                    return 0;
            }
            return 1;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next(1);
        let third = iterator.next(2);
        let fourth = iterator.next();
        first.value + second.value + third.value + fourth.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(43.0));
}

#[test]
fn interpreter_supports_generator_yields_inside_with_statements() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let scope = { value: 40 };
            with (scope) {
                value = value + (yield 2);
            }
            return scope.value;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next(2);
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(44.0));
}

#[test]
fn interpreter_supports_labeled_breaks_in_generators() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let total = 0;
            outer: while (true) {
                total = total + (yield 19);
                break outer;
            }
            return total + 23;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next(19);
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(61.0));
}

#[test]
fn interpreter_supports_labeled_continue_in_generators() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let total = 0;
            outer: for (let i = 0; i < 2; i = i + 1) {
                total = total + 1;
                if (i === 0) {
                    continue outer;
                }
                total = total + (yield 40);
            }
            return total;
        }

        let iterator = values();
        let first = iterator.next();
        let second = iterator.next(40);
        first.value + second.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(82.0));
}

#[test]
fn interpreter_closes_iterators_when_for_of_breaks() {
    let result = eval_with_interpreter(
        r#"
        let closed = 0;
        let source = {
            count: 0,
            next() {
                this.count = this.count + 1;
                if (this.count > 3) {
                    return { done: true };
                }
                return { value: this.count, done: false };
            },
            return() {
                closed = 41;
                return { value: 0, done: true };
            }
        };

        let total = 0;
        for (const value of source) {
            total = total + value;
            break;
        }
        total + closed;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_awaits_values_in_for_await_of_statement() {
    let result = eval_with_interpreter(
        r#"
        let total = 0;
        for await (const value of [Promise.resolve(19), Promise.resolve(23)]) {
            total = total + value;
        }
        total;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_async_iterators_in_for_await_of_statement() {
    let result = eval_with_interpreter(
        r#"
        let source = {
            index: 0,
            next() {
                this.index = this.index + 1;
                if (this.index === 1) {
                    return Promise.resolve({ value: 19, done: false });
                }
                if (this.index === 2) {
                    return Promise.resolve({ value: 23, done: false });
                }
                return Promise.resolve({ done: true });
            }
        };

        let total = 0;
        for await (const value of source) {
            total = total + value;
        }
        total;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_awaits_async_iterator_return_during_for_await_break() {
    let result = eval_with_interpreter(
        r#"
        let closed = 0;
        let source = {
            next() {
                return Promise.resolve({ value: 1, done: false });
            },
            return() {
                return Promise.resolve({ value: closed = 42, done: true });
            }
        };

        for await (const value of source) {
            break;
        }
        closed;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_for_in_over_prototype_properties() {
    let result = eval_with_interpreter(
        r#"
        let proto = { left: 19 };
        let value = { right: 23 };
        value.__proto__ = proto;

        let total = 0;
        for (const key in value) {
            total = total + value[key];
        }
        total;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_for_in_over_strings() {
    let result = eval_with_interpreter(
        r#"
        let total = 2;
        for (const key in "hello") {
            total = total + 8;
        }
        total;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_array_destructuring_from_iterators() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            yield 19;
            yield 23;
        }

        let [left, right] = values();
        left + right;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_spread_from_iterators() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            yield 19;
            yield 23;
        }

        let items = [...values()];
        items[0] + items[1];
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_call_spread_from_iterators() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            yield 19;
            yield 23;
        }

        function add(a, b) {
            return a + b;
        }

        add(...values());
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_imports_default_and_named_exports_from_module_files() {
    let root = write_temp_modules(&[(
        "dep.js",
        r#"
        export default 10;
        export const bonus = 32;
        "#,
    )]);
    let dep = root.join("dep.js");
    let source = format!(
        r#"import value, {{ bonus }} from "{}";
        value + bonus;"#,
        dep.display()
    );

    let result = eval_with_interpreter(&source);

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_keeps_named_import_bindings_live_after_exporter_updates() {
    let root = write_temp_modules(&[(
        "dep.js",
        r#"
        export let value = 1;
        export function setValue(next) {
            value = next;
        }
        "#,
    )]);
    let dep = root.join("dep.js");
    let source = format!(
        r#"import {{ value, setValue }} from "{}";
        setValue(42);
        value;"#,
        dep.display()
    );

    let result = eval_with_interpreter(&source);

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_keeps_default_import_bindings_live_when_default_re_exports_a_binding() {
    let root = write_temp_modules(&[(
        "dep.js",
        r#"
        let value = 1;
        export { value as default, value };
        export function setValue(next) {
            value = next;
        }
        "#,
    )]);
    let dep = root.join("dep.js");
    let source = format!(
        r#"import current, {{ value, setValue }} from "{}";
        setValue(21);
        current + value;"#,
        dep.display()
    );

    let result = eval_with_interpreter(&source);

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_rejects_assignment_to_imported_bindings() {
    let root = write_temp_modules(&[(
        "dep.js",
        r#"
        export let value = 1;
        "#,
    )]);
    let dep = root.join("dep.js");
    let source = format!(
        r#"import {{ value }} from "{}";
        value = 42;"#,
        dep.display()
    );

    let result = eval_with_interpreter_result(&source);

    match result {
        Err(RuntimeError::TypeError(message)) => {
            assert!(message.contains("imported binding"));
        }
        other => panic!("expected imported binding assignment error, got {other:?}"),
    }
}

#[test]
fn interpreter_caches_module_evaluation_results() {
    let root = write_temp_modules(&[(
        "dep.js",
        r#"
        counter = counter + 21;
        export const value = counter;
        "#,
    )]);
    let dep = root.join("dep.js");
    let source = format!(
        r#"let counter = 0;
        import "{}";
        import "{}";
        counter;"#,
        dep.display(),
        dep.display()
    );

    let result = eval_with_interpreter(&source);

    assert_eq!(result, JsValue::Number(21.0));
}

#[test]
fn interpreter_supports_re_export_chains_between_module_files() {
    let root = write_temp_modules(&[
        (
            "dep.js",
            r#"
            export const inner = 19;
            export const extra = 23;
            "#,
        ),
        (
            "mid.js",
            r#"
            export { inner as value } from "./dep.js";
            export * as ns from "./dep.js";
            "#,
        ),
    ]);
    let mid = root.join("mid.js");
    let source = format!(
        r#"import {{ value, ns }} from "{}";
        value + ns.extra;"#,
        mid.display()
    );

    let result = eval_with_interpreter(&source);

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_export_all_from_module_files() {
    let root = write_temp_modules(&[
        (
            "dep.js",
            r#"
            export default 1;
            export const left = 19;
            export const right = 23;
            "#,
        ),
        (
            "mid.js",
            r#"
            export * from "./dep.js";
            "#,
        ),
    ]);
    let mid = root.join("mid.js");
    let source = format!(
        r#"import {{ left, right }} from "{}";
        left + right;"#,
        mid.display()
    );

    let result = eval_with_interpreter(&source);

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_imports_module_namespace_objects() {
    let root = write_temp_modules(&[(
        "dep.js",
        r#"
        export default 2;
        export const value = 40;
        "#,
    )]);
    let dep = root.join("dep.js");
    let source = format!(
        r#"import * as ns from "{}";
        ns.value + ns.default;"#,
        dep.display()
    );

    let result = eval_with_interpreter(&source);

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_keeps_namespace_imports_live_after_module_updates() {
    let root = write_temp_modules(&[(
        "dep.js",
        r#"
        export let value = 1;
        value = 42;
        "#,
    )]);
    let dep = root.join("dep.js");
    let source = format!(
        r#"import * as ns from "{}";
        ns.value;"#,
        dep.display()
    );

    let result = eval_with_interpreter(&source);

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_runs_side_effect_only_imports() {
    let root = write_temp_modules(&[(
        "dep.js",
        r#"
        globalThisValue = 42;
        "#,
    )]);
    let dep = root.join("dep.js");
    let source = format!(
        r#"let globalThisValue = 0;
        import "{}";
        globalThisValue;"#,
        dep.display()
    );

    let result = eval_with_interpreter(&source);

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_keeps_re_exported_namespace_bindings_live() {
    let root = write_temp_modules(&[
        (
            "dep.js",
            r#"
            export let value = 19;
            value = 42;
            "#,
        ),
        (
            "mid.js",
            r#"
            export { value } from "./dep.js";
            "#,
        ),
    ]);
    let mid = root.join("mid.js");
    let source = format!(
        r#"import * as ns from "{}";
        ns.value;"#,
        mid.display()
    );

    let result = eval_with_interpreter(&source);

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_imports_default_exported_functions() {
    let root = write_temp_modules(&[(
        "dep.js",
        r#"
        export default function add(value) {
            return value + 1;
        }
        "#,
    )]);
    let dep = root.join("dep.js");
    let source = format!(
        r#"import add from "{}";
        add(41);"#,
        dep.display()
    );

    let result = eval_with_interpreter(&source);

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_exposes_globalthis_bindings() {
    let result = eval_with_interpreter(
        r#"
        globalThis.answer = 42;
        answer;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_uses_global_object_for_top_level_this() {
    let result = eval_with_interpreter(
        r#"
        this.answer = 42;
        globalThis.answer;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_deletes_globalthis_properties() {
    let result = eval_with_interpreter(
        r#"
        globalThis.answer = 42;
        delete globalThis.answer;
        answer;
        "#,
    );

    assert_eq!(result, JsValue::Undefined);
}

#[test]
fn interpreter_uses_default_parameter_in_object_method() {
    let result = eval_with_interpreter(
        r#"
        let greeter = {
            greet(name = "world") {
                return name;
            }
        };

        greeter.greet();
        "#,
    );

    assert_eq!(result, JsValue::String("world".to_string()));
}

#[test]
fn interpreter_evaluates_object_member_access() {
    let result = eval_with_interpreter(
        r#"
        let value = { foo: 42, "bar": 7 };
        value.foo;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_evaluates_keyword_named_member_access() {
    let result = eval_with_interpreter(
        r#"
        let value = { default: 42 };
        value.default;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_reads_bindings_inside_with_statement() {
    let result = eval_with_interpreter(
        r#"
        let scope = { value: 42 };
        with (scope) {
            value;
        }
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_writes_bindings_inside_with_statement() {
    let result = eval_with_interpreter(
        r#"
        let scope = { value: 41 };
        with (scope) {
            value = value + 1;
        }
        scope.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_labeled_continue_statements() {
    let result = eval_with_interpreter(
        r#"
        let total = 0;
        outer: for (let i = 0; i < 3; i = i + 1) {
            if (i < 2) {
                continue outer;
            }
            total = total + 42;
        }
        total;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_array_destructuring_declaration() {
    let result = eval_with_interpreter(
        r#"
        let [first, , ...rest] = [1, 2, 3, 4];
        first + rest[0] + rest[1];
        "#,
    );

    assert_eq!(result, JsValue::Number(8.0));
}

#[test]
fn interpreter_supports_object_destructuring_declaration() {
    let result = eval_with_interpreter(
        r#"
        let { foo, bar: baz = 10, ...rest } = { foo: 1, qux: 41 };
        foo + baz + rest.qux;
        "#,
    );

    assert_eq!(result, JsValue::Number(52.0));
}

#[test]
fn interpreter_supports_destructuring_assignment() {
    let result = eval_with_interpreter(
        r#"
        let a = 0;
        let b = 0;
        [a, b] = [19, 23];
        a + b;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_for_of_destructuring_binding() {
    let result = eval_with_interpreter(
        r#"
        let total = 0;
        for (let { value, extra = 0 } of [{ value: 19 }, { value: 20, extra: 3 }]) {
            total = total + value + extra;
        }
        total;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_catch_destructuring_parameter() {
    let result = eval_with_interpreter(
        r#"
        try {
            throw { value: 40, bonus: 2 };
        } catch ({ value, bonus }) {
            value + bonus;
        }
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_evaluates_computed_object_member_access() {
    let result = eval_with_interpreter(
        r#"
        let value = { foo: 42 };
        value["foo"];
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_evaluates_array_index_access() {
    let result = eval_with_interpreter(
        r#"
        let value = [1, 2, 3];
        value[1];
        "#,
    );

    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn interpreter_keeps_array_holes_as_undefined() {
    let result = eval_with_interpreter(
        r#"
        let value = [1, , 3];
        value[1];
        "#,
    );

    assert_eq!(result, JsValue::Undefined);
}

#[test]
fn interpreter_returns_undefined_for_missing_members() {
    let result = eval_with_interpreter(
        r#"
        let value = { foo: 42 };
        value.missing;
        "#,
    );

    assert_eq!(result, JsValue::Undefined);
}

#[test]
fn interpreter_returns_array_length() {
    let result = eval_with_interpreter(
        r#"
        let value = [1, 2, 3];
        value.length;
        "#,
    );

    assert_eq!(result, JsValue::Number(3.0));
}

#[test]
fn interpreter_errors_on_primitive_member_access() {
    let error = eval_with_interpreter_result("null.foo;").expect_err("member access should fail");

    assert!(matches!(error, RuntimeError::TypeError(_)));
}

#[test]
fn interpreter_assigns_object_member_with_dot_syntax() {
    let result = eval_with_interpreter(
        r#"
        let obj = { foo: 0 };
        obj.foo = 1;
        obj.foo;
        "#,
    );

    assert_eq!(result, JsValue::Number(1.0));
}

#[test]
fn interpreter_assigns_object_member_with_computed_syntax() {
    let result = eval_with_interpreter(
        r#"
        let obj = {};
        obj["foo"] = 2;
        obj.foo;
        "#,
    );

    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn interpreter_assigns_array_member_by_index() {
    let result = eval_with_interpreter(
        r#"
        let arr = [0, 1];
        arr[0] = 7;
        arr[0];
        "#,
    );

    assert_eq!(result, JsValue::Number(7.0));
}

#[test]
fn interpreter_extends_array_on_out_of_bounds_assignment() {
    let result = eval_with_interpreter(
        r#"
        let arr = [];
        arr[2] = 9;
        arr[2];
        "#,
    );

    assert_eq!(result, JsValue::Number(9.0));
}

#[test]
fn interpreter_updates_array_length_after_assignment() {
    let result = eval_with_interpreter(
        r#"
        let arr = [];
        arr[2] = 9;
        arr.length;
        "#,
    );

    assert_eq!(result, JsValue::Number(3.0));
}

#[test]
fn interpreter_assigns_nested_members() {
    let result = eval_with_interpreter(
        r#"
        let obj = { inner: { value: 0 } };
        obj.inner.value = 5;
        obj.inner.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(5.0));
}

#[test]
fn interpreter_shares_object_references_between_aliases() {
    let result = eval_with_interpreter(
        r#"
        let obj = { x: 0 };
        let alias = obj;
        alias.x = 3;
        obj.x;
        "#,
    );

    assert_eq!(result, JsValue::Number(3.0));
}

#[test]
fn interpreter_errors_on_primitive_member_assignment() {
    let error =
        eval_with_interpreter_result("null.foo = 1;").expect_err("member assignment should fail");

    assert!(matches!(error, RuntimeError::TypeError(_)));
}

#[test]
fn interpreter_deletes_object_members() {
    let result = eval_with_interpreter(
        r#"
        let obj = { value: 1 };
        delete obj.value;
        obj.value;
        "#,
    );

    assert_eq!(result, JsValue::Undefined);
}

#[test]
fn interpreter_delete_returns_false_for_identifiers() {
    let result = eval_with_interpreter(
        r#"
        let value = 1;
        delete value;
        "#,
    );

    assert_eq!(result, JsValue::Boolean(false));
}

#[test]
fn interpreter_deletes_array_indices_without_shrinking_length() {
    let result = eval_with_interpreter(
        r#"
        let arr = [1, 2, 3];
        delete arr[1];
        arr.length + arr[1];
        "#,
    );

    assert!(matches!(result, JsValue::Number(n) if n.is_nan()));
}

#[test]
fn interpreter_applies_plus_assign_to_identifier() {
    let result = eval_with_interpreter(
        r#"
        let x = 1;
        x += 4;
        x;
        "#,
    );

    assert_eq!(result, JsValue::Number(5.0));
}

#[test]
fn interpreter_applies_percent_assign_to_identifier() {
    let result = eval_with_interpreter(
        r#"
        let x = 6;
        x %= 4;
        x;
        "#,
    );

    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn interpreter_applies_power_assign_to_identifier() {
    let result = eval_with_interpreter(
        r#"
        let x = 2;
        x **= 5;
        x;
        "#,
    );

    assert_eq!(result, JsValue::Number(32.0));
}

#[test]
fn interpreter_applies_compound_assignment_to_object_member() {
    let result = eval_with_interpreter(
        r#"
        let obj = { x: 3 };
        obj.x *= 2;
        obj.x;
        "#,
    );

    assert_eq!(result, JsValue::Number(6.0));
}

#[test]
fn interpreter_applies_compound_assignment_to_array_member() {
    let result = eval_with_interpreter(
        r#"
        let arr = [5];
        arr[0] -= 2;
        arr[0];
        "#,
    );

    assert_eq!(result, JsValue::Number(3.0));
}

#[test]
fn interpreter_applies_compound_assignment_to_nested_member() {
    let result = eval_with_interpreter(
        r#"
        let obj = { inner: { value: 1 } };
        obj.inner.value += 4;
        obj.inner.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(5.0));
}

#[test]
fn interpreter_returns_compound_assignment_result() {
    let result = eval_with_interpreter(
        r#"
        let x = 1;
        let y = (x += 3);
        y;
        "#,
    );

    assert_eq!(result, JsValue::Number(4.0));
}

#[test]
fn interpreter_errors_on_array_length_compound_assignment() {
    let error = eval_with_interpreter_result(
        r#"
        let arr = [1];
        arr.length += 1;
        "#,
    )
    .expect_err("array length compound assignment should fail");

    assert!(matches!(error, RuntimeError::TypeError(_)));
}

#[test]
fn interpreter_applies_logical_or_assign_to_identifier() {
    let result = eval_with_interpreter(
        r#"
        let x = 0;
        x ||= 5;
        x;
        "#,
    );

    assert_eq!(result, JsValue::Number(5.0));
}

#[test]
fn interpreter_short_circuits_logical_or_assign() {
    let result = eval_with_interpreter(
        r#"
        let x = 1;
        x ||= 5;
        x;
        "#,
    );

    assert_eq!(result, JsValue::Number(1.0));
}

#[test]
fn interpreter_applies_logical_and_assign_to_identifier() {
    let result = eval_with_interpreter(
        r#"
        let x = 1;
        x &&= 5;
        x;
        "#,
    );

    assert_eq!(result, JsValue::Number(5.0));
}

#[test]
fn interpreter_short_circuits_logical_and_assign() {
    let result = eval_with_interpreter(
        r#"
        let x = 0;
        x &&= 5;
        x;
        "#,
    );

    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn interpreter_applies_nullish_assign_to_undefined_identifier() {
    let result = eval_with_interpreter(
        r#"
        let x;
        x ??= 5;
        x;
        "#,
    );

    assert_eq!(result, JsValue::Number(5.0));
}

#[test]
fn interpreter_applies_nullish_assign_to_null_identifier() {
    let result = eval_with_interpreter(
        r#"
        let x = null;
        x ??= 5;
        x;
        "#,
    );

    assert_eq!(result, JsValue::Number(5.0));
}

#[test]
fn interpreter_does_not_apply_nullish_assign_to_zero() {
    let result = eval_with_interpreter(
        r#"
        let x = 0;
        x ??= 5;
        x;
        "#,
    );

    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn interpreter_applies_logical_assignment_to_members() {
    let result = eval_with_interpreter(
        r#"
        let obj = { x: 0, y: 1 };
        obj.x ||= 4;
        obj.y &&= 6;
        obj.y;
        "#,
    );

    assert_eq!(result, JsValue::Number(6.0));
}

#[test]
fn interpreter_applies_nullish_assign_to_array_index() {
    let result = eval_with_interpreter(
        r#"
        let arr = [];
        arr[0] ??= 7;
        arr[0];
        "#,
    );

    assert_eq!(result, JsValue::Number(7.0));
}

#[test]
fn interpreter_errors_on_array_length_logical_assignment() {
    let error = eval_with_interpreter_result(
        r#"
        let arr = [];
        arr.length ||= 2;
        "#,
    )
    .expect_err("array length logical assignment should fail");

    assert!(matches!(error, RuntimeError::TypeError(_)));
}

#[test]
fn interpreter_short_circuits_member_logical_assignment_rhs() {
    let result = eval_with_interpreter(
        r#"
        let obj = { x: 1 };
        let y = 0;
        obj.x ||= (y = 1);
        y;
        "#,
    );

    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn interpreter_generator_short_circuits_member_logical_assignment_rhs() {
    let result = eval_with_interpreter(
        r#"
        function* values() {
            let obj = { x: 1 };
            let y = 0;
            obj.x ||= (y = yield 1);
            return y;
        }

        let iterator = values();
        let first = iterator.next();
        first.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn interpreter_errors_on_invalid_assignment_target() {
    let lexer = Lexer::new("(1 + 2) = 3;");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("invalid assignment target should fail");

    assert!(format!("{error}").contains("Invalid assignment target"));
}

#[test]
fn interpreter_evaluates_destructuring_member_target_object_before_key() {
    let result = eval_with_interpreter(
        r#"
        let log = "";
        function getObject() {
            log = log + "obj,";
            return {
                set value(v) {
                    log = log + "set:" + v;
                }
            };
        }
        function getKey() {
            log = log + "key,";
            return "value";
        }
        [getObject()[getKey()]] = [1];
        log;
        "#,
    );

    assert_eq!(result, JsValue::String("obj,key,set:1".to_string()));
}

#[test]
fn interpreter_short_circuits_super_logical_assignment_rhs() {
    let result = eval_with_interpreter(
        r#"
        class Base {
            get value() { return 1; }
            set value(v) { this.answer = v; }
        }
        class Foo extends Base {
            run() {
                let side = 0;
                super.value ||= (side = 1);
                return side;
            }
        }
        new Foo().run();
        "#,
    );

    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn interpreter_applies_super_nullish_assignment_through_setter() {
    let result = eval_with_interpreter(
        r#"
        class Base {
            get value() { return undefined; }
            set value(v) { this.answer = v + 1; }
        }
        class Foo extends Base {
            run() {
                super.value ??= 41;
                return this.answer;
            }
        }
        new Foo().run();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_short_circuits_private_logical_assignment_rhs() {
    let result = eval_with_interpreter(
        r#"
        class Box {
            #value = 1;
            run() {
                let side = 0;
                this.#value ||= (side = 1);
                return side;
            }
        }
        new Box().run();
        "#,
    );

    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn interpreter_applies_private_accessor_logical_assignment_once() {
    let result = eval_with_interpreter(
        r#"
        class Box {
            #value = 0;
            #reads = 0;
            #writes = 0;
            get #current() {
                this.#reads += 1;
                return this.#value;
            }
            set #current(v) {
                this.#writes += 1;
                this.#value = v;
            }
            run() {
                this.#current ||= 42;
                return this.#reads * 100 + this.#writes * 10 + this.#value;
            }
        }
        new Box().run();
        "#,
    );

    assert_eq!(result, JsValue::Number(52.0));
}

#[test]
fn interpreter_generator_short_circuits_private_nullish_assignment_rhs() {
    let result = eval_with_interpreter(
        r#"
        class Box {
            #value = 1;
            *run() {
                this.#value ??= yield 1;
                return this.#value;
            }
        }
        let iterator = new Box().run();
        let first = iterator.next();
        first.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(1.0));
}

#[test]
fn interpreter_evaluates_bitwise_binary_or() {
    let result = eval_with_interpreter("5 | 2;");
    assert_eq!(result, JsValue::Number(7.0));
}

#[test]
fn interpreter_evaluates_bitwise_binary_xor() {
    let result = eval_with_interpreter("5 ^ 1;");
    assert_eq!(result, JsValue::Number(4.0));
}

#[test]
fn interpreter_evaluates_bitwise_binary_shift_left() {
    let result = eval_with_interpreter("8 << 1;");
    assert_eq!(result, JsValue::Number(16.0));
}

#[test]
fn interpreter_evaluates_bitwise_binary_shift_right() {
    let result = eval_with_interpreter("8 >> 1;");
    assert_eq!(result, JsValue::Number(4.0));
}

#[test]
fn interpreter_evaluates_bitwise_binary_unsigned_shift_right() {
    let result = eval_with_interpreter("-1 >>> 1;");
    assert_eq!(result, JsValue::Number(2147483647.0));
}

#[test]
fn interpreter_applies_bitand_assign_to_identifier() {
    let result = eval_with_interpreter(
        r#"
        let x = 7;
        x &= 3;
        x;
        "#,
    );

    assert_eq!(result, JsValue::Number(3.0));
}

#[test]
fn interpreter_applies_bitor_assign_to_object_member() {
    let result = eval_with_interpreter(
        r#"
        let obj = { x: 1 };
        obj.x |= 4;
        obj.x;
        "#,
    );

    assert_eq!(result, JsValue::Number(5.0));
}

#[test]
fn interpreter_applies_bitxor_assign_to_array_member() {
    let result = eval_with_interpreter(
        r#"
        let arr = [5];
        arr[0] ^= 1;
        arr[0];
        "#,
    );

    assert_eq!(result, JsValue::Number(4.0));
}

#[test]
fn interpreter_applies_shift_left_assign() {
    let result = eval_with_interpreter(
        r#"
        let x = 2;
        x <<= 2;
        x;
        "#,
    );

    assert_eq!(result, JsValue::Number(8.0));
}

#[test]
fn interpreter_applies_unsigned_shift_right_assign() {
    let result = eval_with_interpreter(
        r#"
        let x = -1;
        x >>>= 1;
        x;
        "#,
    );

    assert_eq!(result, JsValue::Number(2147483647.0));
}

#[test]
fn interpreter_returns_bitwise_assignment_result() {
    let result = eval_with_interpreter(
        r#"
        let x = 7;
        let y = (x &= 6);
        y;
        "#,
    );

    assert_eq!(result, JsValue::Number(6.0));
}

#[test]
fn interpreter_errors_on_array_length_bitwise_assignment() {
    let error = eval_with_interpreter_result(
        r#"
        let arr = [1];
        arr.length &= 1;
        "#,
    )
    .expect_err("array length bitwise assignment should fail");

    assert!(matches!(error, RuntimeError::TypeError(_)));
}

#[test]
fn interpreter_evaluates_unary_bitnot_on_number() {
    let result = eval_with_interpreter("~5;");
    assert_eq!(result, JsValue::Number(-6.0));
}

#[test]
fn interpreter_evaluates_unary_bitnot_on_negative_one() {
    let result = eval_with_interpreter("~-1;");
    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn interpreter_evaluates_unary_bitnot_on_true() {
    let result = eval_with_interpreter("~true;");
    assert_eq!(result, JsValue::Number(-2.0));
}

#[test]
fn interpreter_evaluates_unary_bitnot_on_zero() {
    let result = eval_with_interpreter("~0;");
    assert_eq!(result, JsValue::Number(-1.0));
}

#[test]
fn interpreter_evaluates_nullish_coalescing_with_undefined() {
    let result = eval_with_interpreter("undefined ?? 5;");
    assert_eq!(result, JsValue::Number(5.0));
}

#[test]
fn interpreter_evaluates_nullish_coalescing_with_null() {
    let result = eval_with_interpreter("null ?? 5;");
    assert_eq!(result, JsValue::Number(5.0));
}

#[test]
fn interpreter_does_not_treat_zero_as_nullish() {
    let result = eval_with_interpreter("0 ?? 5;");
    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn interpreter_evaluates_bitwise_binary_and_on_null() {
    let result = eval_with_interpreter("null & 1;");
    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn interpreter_evaluates_bitwise_binary_or_on_undefined() {
    let result = eval_with_interpreter("undefined | 1;");
    assert_eq!(result, JsValue::Number(1.0));
}

#[test]
fn interpreter_evaluates_unary_bitnot_on_null() {
    let result = eval_with_interpreter("~null;");
    assert_eq!(result, JsValue::Number(-1.0));
}

#[test]
fn interpreter_evaluates_unary_bitnot_on_undefined() {
    let result = eval_with_interpreter("~undefined;");
    assert_eq!(result, JsValue::Number(-1.0));
}

#[test]
fn interpreter_evaluates_await_expression() {
    let result = eval_with_interpreter(
        r#"
        let value = 41;
        await value + 1;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_evaluates_yield_expression() {
    let result = eval_with_interpreter(
        r#"
        let value = 41;
        yield value + 1;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_evaluates_unary_bitnot_on_string_number() {
    let result = eval_with_interpreter("~'5';");
    assert_eq!(result, JsValue::Number(-6.0));
}

#[test]
fn interpreter_binds_this_for_member_calls() {
    let result = eval_with_interpreter(
        r#"
        let obj = {
            value: 41,
            inc() { return this.value + 1; }
        };
        obj.inc();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_constructs_instances_with_new() {
    let result = eval_with_interpreter(
        r#"
        function Foo(value) {
            this.value = value;
        }
        let foo = new Foo(42);
        foo.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_new_returns_explicit_object() {
    let result = eval_with_interpreter(
        r#"
        function Foo() {
            this.value = 1;
            return { value: 42 };
        }
        let foo = new Foo();
        foo.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_new_returns_explicit_array_object() {
    let result = eval_with_interpreter(
        r#"
        function Foo() {
            return [42];
        }
        let value = new Foo();
        value[0];
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_new_ignores_primitive_return_value() {
    let result = eval_with_interpreter(
        r#"
        function Foo() {
            this.value = 42;
            return 1;
        }
        let foo = new Foo();
        foo.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_evaluates_instanceof_with_constructed_objects() {
    let result = eval_with_interpreter(
        r#"
        function Foo() {}
        let foo = new Foo();
        foo instanceof Foo;
        "#,
    );

    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn interpreter_constructs_class_instances() {
    let result = eval_with_interpreter(
        r#"
        class Foo {
            constructor(value) { this.value = value; }
            bar() { return this.value; }
        }
        new Foo(42).bar();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_rejects_new_on_object_literal_methods() {
    let error = eval_with_interpreter_result(
        r#"
        let obj = {
            method() { return 42; }
        };
        new obj.method();
        "#,
    )
    .expect_err("object literal methods should not be constructible");

    assert!(matches!(error, RuntimeError::TypeError(_)));
    assert!(format!("{error}").contains("not a constructor"));
}

#[test]
fn interpreter_object_literal_methods_do_not_expose_prototype() {
    let result = eval_with_interpreter(
        r#"
        let obj = {
            method() { return 42; }
        };
        obj.method.prototype;
        "#,
    );

    assert_eq!(result, JsValue::Undefined);
}

#[test]
fn interpreter_rejects_calling_class_constructor_without_new() {
    let error = eval_with_interpreter_result(
        r#"
        class Foo {
            constructor() { this.value = 42; }
        }
        Foo();
        "#,
    )
    .expect_err("class constructor call without new should fail");

    assert!(matches!(error, RuntimeError::TypeError(_)));
    assert!(format!("{error}").contains("class constructor cannot be invoked without 'new'"));
}

#[test]
fn interpreter_rejects_calling_derived_class_constructor_without_new() {
    let error = eval_with_interpreter_result(
        r#"
        class Base {}
        class Foo extends Base {}
        Foo();
        "#,
    )
    .expect_err("derived class constructor call without new should fail");

    assert!(matches!(error, RuntimeError::TypeError(_)));
    assert!(format!("{error}").contains("class constructor cannot be invoked without 'new'"));
}

#[test]
fn interpreter_uses_default_parameter_in_class_method() {
    let result = eval_with_interpreter(
        r#"
        class Greeter {
            greet(name = "world") { return name; }
        }
        new Greeter().greet();
        "#,
    );

    assert_eq!(result, JsValue::String("world".to_string()));
}

#[test]
fn interpreter_rejects_new_on_class_methods() {
    let error = eval_with_interpreter_result(
        r#"
        class Foo {
            method() { return 42; }
        }
        let foo = new Foo();
        new foo.method();
        "#,
    )
    .expect_err("class methods should not be constructible");

    assert!(matches!(error, RuntimeError::TypeError(_)));
    assert!(format!("{error}").contains("not a constructor"));
}

#[test]
fn interpreter_class_methods_do_not_expose_prototype() {
    let result = eval_with_interpreter(
        r#"
        class Foo {
            method() { return 42; }
        }
        let foo = new Foo();
        foo.method.prototype;
        "#,
    );

    assert_eq!(result, JsValue::Undefined);
}

#[test]
fn interpreter_supports_class_extends_and_super_call() {
    let result = eval_with_interpreter(
        r#"
        class Base {
            constructor(value) { this.value = value; }
        }
        class Foo extends Base {
            constructor() { super(42); }
        }
        new Foo().value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_static_super_method_calls() {
    let result = eval_with_interpreter(
        r#"
        class Base {
            static value() { return 41; }
        }
        class Foo extends Base {
            static value() { return super.value() + 1; }
        }
        Foo.value();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_allows_derived_constructor_to_return_object_without_super() {
    let result = eval_with_interpreter(
        r#"
        class Base {}
        class Foo extends Base {
            constructor() {
                return { value: 42 };
            }
        }
        let foo = new Foo();
        foo.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_super_method_calls() {
    let result = eval_with_interpreter(
        r#"
        class Base {
            bar() { return 41; }
        }
        class Foo extends Base {
            bar() { return super.bar() + 1; }
        }
        new Foo().bar();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_writes_super_data_properties_to_the_receiver() {
    let result = eval_with_interpreter(
        r#"
        class Base {}
        Base.prototype.value = 1;
        class Foo extends Base {
            write() {
                super.value = 42;
                return this.value + Base.prototype.value;
            }
        }
        new Foo().write();
        "#,
    );

    assert_eq!(result, JsValue::Number(43.0));
}

#[test]
fn interpreter_supports_super_property_setters() {
    let result = eval_with_interpreter(
        r#"
        class Base {
            set value(v) { this.answer = v + 1; }
        }
        class Foo extends Base {
            write() {
                super.value = 41;
                return this.answer;
            }
        }
        new Foo().write();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_super_computed_property_setters_in_object_literals() {
    let result = eval_with_interpreter(
        r#"
        let base = {
            set value(v) { this.answer = v + 1; }
        };
        let obj = {
            __proto__: base,
            write() {
                super["value"] = 41;
                return this.answer;
            }
        };
        obj.write();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_preserves_super_write_receiver_for_borrowed_methods() {
    let result = eval_with_interpreter(
        r#"
        let base = {
            set value(v) { this.answer = v + 1; }
        };
        let obj = {
            __proto__: base,
            write(v) {
                super.value = v;
                return this.answer;
            }
        };
        let other = {
            answer: 0,
            method: obj.write
        };
        other.method(41);
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_super_compound_assignment() {
    let result = eval_with_interpreter(
        r#"
        class Base {
            get count() { return this._count; }
            set count(v) { this._count = v; }
        }
        class Foo extends Base {
            constructor() {
                super();
                this._count = 41;
            }
            inc() {
                super.count += 1;
                return this._count;
            }
        }
        new Foo().inc();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_super_update_expressions() {
    let result = eval_with_interpreter(
        r#"
        class Base {
            get count() { return this._count; }
            set count(v) { this._count = v; }
        }
        class Foo extends Base {
            constructor() {
                super();
                this._count = 41;
            }
            inc() {
                let previous = super.count++;
                return previous + this._count;
            }
        }
        new Foo().inc();
        "#,
    );

    assert_eq!(result, JsValue::Number(83.0));
}

#[test]
fn interpreter_supports_super_destructuring_assignment_targets() {
    let result = eval_with_interpreter(
        r#"
        class Base {
            set value(v) { this.answer = v + 1; }
        }
        class Foo extends Base {
            write() {
                [super.value] = [41];
                return this.answer;
            }
        }
        new Foo().write();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_generator_super_compound_assignments_with_yielded_keys() {
    let result = eval_with_interpreter(
        r#"
        class Base {
            get count() { return this._count; }
            set count(v) { this._count = v; }
        }
        class Foo extends Base {
            constructor() {
                super();
                this._count = 41;
            }
            *inc() {
                super[yield "count"] += yield 1;
                return this._count;
            }
        }
        let iterator = new Foo().inc();
        let first = iterator.next();
        let second = iterator.next("count");
        let third = iterator.next(1);
        first.value + ":" + second.value + ":" + third.value;
        "#,
    );

    assert_eq!(result, JsValue::String("count:1:42".to_string()));
}

#[test]
fn interpreter_supports_super_method_calls_in_object_literals() {
    let result = eval_with_interpreter(
        r#"
        let base = {
            value() { return 41; }
        };
        let obj = {
            __proto__: base,
            value() { return super.value() + 1; }
        };
        obj.value();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_super_computed_method_calls_in_object_literals() {
    let result = eval_with_interpreter(
        r#"
        let base = {
            value() { return 41; }
        };
        let obj = {
            __proto__: base,
            answer() { return super["value"]() + 1; }
        };
        obj.answer();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_super_getters_in_object_literals() {
    let result = eval_with_interpreter(
        r#"
        let base = {
            get value() { return 41; }
        };
        let obj = {
            __proto__: base,
            get answer() { return super.value + 1; }
        };
        obj.answer;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_preserves_object_literal_super_home_object_when_method_is_borrowed() {
    let result = eval_with_interpreter(
        r#"
        let base = {
            value() { return this.seed + 1; }
        };
        let obj = {
            __proto__: base,
            value() { return super.value() + 1; }
        };
        let other = {
            seed: 40,
            method: obj.value
        };
        other.method();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_resolves_object_literal_super_after_proto_is_defined_later() {
    let result = eval_with_interpreter(
        r#"
        let base = {
            value() { return 41; }
        };
        let obj = {
            value() { return super.value() + 1; },
            __proto__: base
        };
        obj.value();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_keeps_instanceof_working_for_classes() {
    let result = eval_with_interpreter(
        r#"
        class Base {}
        class Foo extends Base {}
        let foo = new Foo();
        foo instanceof Base;
        "#,
    );

    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn interpreter_uses_default_derived_constructor() {
    let result = eval_with_interpreter(
        r#"
        class Base {
            constructor(value) { this.value = value; }
        }
        class Foo extends Base {}
        new Foo(42).value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_super_computed_method_calls() {
    let result = eval_with_interpreter(
        r#"
        class Base {
            bar() { return 41; }
        }
        class Foo extends Base {
            baz() { return super["bar"]() + 1; }
        }
        new Foo().baz();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_errors_when_accessing_this_before_super() {
    let error = eval_with_interpreter_result(
        r#"
        class Base {}
        class Foo extends Base {
            constructor() {
                this.value = 1;
                super();
            }
        }
        new Foo();
        "#,
    )
    .expect_err("derived constructor should require super before this");

    assert!(matches!(error, RuntimeError::TypeError(_)));
}

#[test]
fn interpreter_errors_when_super_is_used_outside_method_context() {
    let error =
        eval_with_interpreter_result("super();").expect_err("top-level super call should fail");

    assert!(matches!(error, RuntimeError::TypeError(_)));
}

#[test]
fn interpreter_supports_static_method_calls() {
    let result = eval_with_interpreter(
        r#"
        class Foo {
            static bar() { return 42; }
        }
        Foo.bar();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_static_fields() {
    let result = eval_with_interpreter(
        r#"
        class Foo {
            static value = 42;
        }
        Foo.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_instance_fields() {
    let result = eval_with_interpreter(
        r#"
        class Foo {
            value = 42;
        }
        new Foo().value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_initializes_instance_fields_in_order() {
    let result = eval_with_interpreter(
        r#"
        class Foo {
            first = 20;
            second = this.first + 22;
        }
        new Foo().second;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_initializes_derived_instance_fields_after_super() {
    let result = eval_with_interpreter(
        r#"
        class Base {}
        class Foo extends Base {
            value = 41;
            constructor() {
                super();
                this.value = this.value + 1;
            }
        }
        new Foo().value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_super_in_instance_field_initializers() {
    let result = eval_with_interpreter(
        r#"
        class Base {
            get value() { return 41; }
        }
        class Foo extends Base {
            answer = super.value + 1;
        }
        new Foo().answer;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_does_not_expose_static_methods_on_instances() {
    let result = eval_with_interpreter(
        r#"
        class Foo {
            static bar() { return 42; }
        }
        let foo = new Foo();
        foo.bar;
        "#,
    );

    assert_eq!(result, JsValue::Undefined);
}

#[test]
fn interpreter_inherits_static_methods_through_extends() {
    let result = eval_with_interpreter(
        r#"
        class Base {
            static bar() { return 42; }
        }
        class Foo extends Base {}
        Foo.bar();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_initializes_static_fields_in_order() {
    let result = eval_with_interpreter(
        r#"
        let history = [];
        class Foo {
            static a = history[history.length] = 1;
            static b = history[history.length] = 2;
        }
        history[0] + history[1];
        "#,
    );

    assert_eq!(result, JsValue::Number(3.0));
}

#[test]
fn interpreter_reads_object_getter() {
    let result = eval_with_interpreter(
        r#"
        let obj = {
            inner: 42,
            get value() { return this.inner; }
        };
        obj.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_writes_object_setter() {
    let result = eval_with_interpreter(
        r#"
        let obj = {
            inner: 0,
            set value(v) { this.inner = v; }
        };
        obj.value = 42;
        obj.inner;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_reads_class_getter() {
    let result = eval_with_interpreter(
        r#"
        class Foo {
            constructor() { this.inner = 42; }
            get value() { return this.inner; }
        }
        new Foo().value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_writes_class_setter() {
    let result = eval_with_interpreter(
        r#"
        class Foo {
            constructor() { this.inner = 0; }
            set value(v) { this.inner = v; }
        }
        let foo = new Foo();
        foo.value = 42;
        foo.inner;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_reads_static_getter() {
    let result = eval_with_interpreter(
        r#"
        class Foo {
            static get value() { return 42; }
        }
        Foo.value;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_writes_static_setter() {
    let result = eval_with_interpreter(
        r#"
        class Foo {
            static inner = 0;
            static set value(v) { this.inner = v; }
        }
        Foo.value = 42;
        Foo.inner;
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_short_circuits_optional_member_on_null() {
    let result = eval_with_interpreter("null?.foo;");
    assert_eq!(result, JsValue::Undefined);
}

#[test]
fn interpreter_short_circuits_optional_computed_member_on_undefined() {
    let result = eval_with_interpreter("undefined?.[0];");
    assert_eq!(result, JsValue::Undefined);
}

#[test]
fn interpreter_evaluates_optional_member_on_defined_value() {
    let result = eval_with_interpreter("({ foo: 42 })?.foo;");
    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_short_circuits_optional_call_on_undefined() {
    let result = eval_with_interpreter(
        r#"
        let fnRef = undefined;
        fnRef?.();
        "#,
    );
    assert_eq!(result, JsValue::Undefined);
}

#[test]
fn interpreter_preserves_this_for_optional_method_calls() {
    let result = eval_with_interpreter(
        r#"
        let obj = {
            value: 41,
            inc() { return this.value + 1; }
        };
        obj?.inc();
        "#,
    );
    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_creates_regexp_literal_object() {
    let result = eval_with_interpreter("/foo/;");
    match result {
        JsValue::Object(_) => {}
        other => panic!("expected regexp object, got {other:?}"),
    }
}

#[test]
fn interpreter_exposes_regexp_source() {
    let result = eval_with_interpreter("/foo/.source;");
    assert_eq!(result, JsValue::String("foo".to_string()));
}

#[test]
fn interpreter_exposes_regexp_flags() {
    let result = eval_with_interpreter("/foo/gi.flags;");
    assert_eq!(result, JsValue::String("gi".to_string()));
}

#[test]
fn interpreter_evaluates_template_literal() {
    let result = eval_with_interpreter(
        r#"
        let value = 41;
        `answer: ${value + 1}`;
        "#,
    );

    assert_eq!(result, JsValue::String("answer: 42".to_string()));
}

#[test]
fn interpreter_evaluates_tagged_template_expression() {
    let result = eval_with_interpreter(
        r#"
        function tag(strings, value) {
            return strings[0] + value + strings[1];
        }
        tag`answer: ${41 + 1}!`;
        "#,
    );

    assert_eq!(result, JsValue::String("answer: 42!".to_string()));
}

#[test]
fn interpreter_binds_this_for_tagged_template_member_calls() {
    let result = eval_with_interpreter(
        r#"
        let object = {
            prefix: "answer: ",
            tag(strings, value) {
                return this.prefix + value + strings[1];
            }
        };
        object.tag`${41 + 1}!`;
        "#,
    );

    assert_eq!(result, JsValue::String("answer: 42!".to_string()));
}

#[test]
fn interpreter_supports_private_instance_fields_and_methods() {
    let result = eval_with_interpreter(
        r#"
        class Counter {
            #value = 40;
            #inc() { this.#value += 2; }
            run() {
                this.#inc();
                return this.#value;
            }
        }

        new Counter().run();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_private_accessors() {
    let result = eval_with_interpreter(
        r#"
        class Counter {
            #value = 40;
            get #current() { return this.#value; }
            set #current(value) { this.#value = value; }
            run() {
                this.#current = this.#current + 2;
                return this.#value;
            }
        }

        new Counter().run();
        "#,
    );

    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_static_private_members_and_brand_checks() {
    let result = eval_with_interpreter(
        r#"
        class Counter {
            static #count = 41;
            static #next() { this.#count++; return this.#count; }
            static hasBrand(value) { return #count in value; }
            static run() { return [this.hasBrand(this), this.#next()]; }
        }

        Counter.run();
        "#,
    );

    let JsValue::Array(values) = result else {
        panic!("expected array result");
    };
    let values = values.borrow();
    assert_eq!(values.len(), 2);
    assert_eq!(values[0], JsValue::Boolean(true));
    assert_eq!(values[1], JsValue::Number(42.0));
}

#[test]
fn interpreter_supports_private_destructuring_and_generator_updates() {
    let result = eval_with_interpreter(
        r#"
        class Box {
            #value = 0;
            *run() {
                [this.#value] = [yield 41];
                yield this.#value++;
                return this.#value;
            }
        }

        let iter = new Box().run();
        let first = iter.next();
        let second = iter.next(first.value + 1);
        let third = iter.next();
        [first.value, second.value, third.value];
        "#,
    );

    let JsValue::Array(values) = result else {
        panic!("expected array result");
    };
    let values = values.borrow();
    assert_eq!(values.len(), 3);
    assert_eq!(values[0], JsValue::Number(41.0));
    assert_eq!(values[1], JsValue::Number(42.0));
    assert_eq!(values[2], JsValue::Number(43.0));
}

#[test]
fn interpreter_rejects_private_access_on_wrong_brand() {
    let error = eval_with_interpreter_result(
        r#"
        class Foo {
            #value = 1;
            read(other) { return other.#value; }
        }

        new Foo().read({});
        "#,
    )
    .expect_err("private access on wrong brand should fail");

    assert!(matches!(error, RuntimeError::TypeError(_)));
}

#[test]
fn interpreter_rejects_undeclared_private_names_in_class_bodies() {
    let error = eval_with_interpreter_result(
        r#"
        class Foo {
            method() {
                return this.#missing;
            }
        }

        new Foo();
        "#,
    )
    .expect_err("undeclared private name should fail");

    assert!(matches!(error, RuntimeError::SyntaxError(_)));
}

#[test]
fn interpreter_rejects_duplicate_private_names_across_static_and_instance_members() {
    let error = eval_with_interpreter_result(
        r#"
        class Foo {
            static #value = 1;
            #value = 2;
        }

        Foo;
        "#,
    )
    .expect_err("duplicate private name should fail");

    assert!(matches!(error, RuntimeError::SyntaxError(_)));
}

#[test]
fn interpreter_rejects_duplicate_private_getters() {
    let error = eval_with_interpreter_result(
        r#"
        class Bar {
            get #value() { return 1; }
            get #value() { return 2; }
        }

        Bar;
        "#,
    )
    .expect_err("duplicate private getter should fail");

    assert!(matches!(error, RuntimeError::SyntaxError(_)));
}
