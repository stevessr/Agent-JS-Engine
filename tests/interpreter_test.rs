use ai_agent::engine::{Interpreter, JsValue};
use ai_agent::lexer::Lexer;
use ai_agent::parser::Parser;

fn eval_with_interpreter(source: &str) -> JsValue {
    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");
    let mut interpreter = Interpreter::new();
    interpreter
        .eval_program(&program)
        .expect("program should evaluate")
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
