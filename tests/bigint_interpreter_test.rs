use ai_agent::engine::interpreter::RuntimeError;
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

fn eval_with_interpreter_result(source: &str) -> Result<JsValue, RuntimeError> {
    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");
    let mut interpreter = Interpreter::new();
    interpreter.eval_program(&program)
}

#[test]
fn interpreter_evaluates_bigint_literal() {
    let result = eval_with_interpreter("123n");
    assert_eq!(result, JsValue::BigInt(123));
}

#[test]
fn interpreter_evaluates_bigint_hex() {
    let result = eval_with_interpreter("0xFFn");
    assert_eq!(result, JsValue::BigInt(255));
}

#[test]
fn interpreter_evaluates_bigint_with_numeric_separators() {
    let result = eval_with_interpreter("1_000n");
    assert_eq!(result, JsValue::BigInt(1000));
}

#[test]
fn interpreter_adds_bigints() {
    let result = eval_with_interpreter("123n + 2n");
    assert_eq!(result, JsValue::BigInt(125));
}

#[test]
fn interpreter_subtracts_bigints() {
    let result = eval_with_interpreter("10n - 3n");
    assert_eq!(result, JsValue::BigInt(7));
}

#[test]
fn interpreter_multiplies_bigints() {
    let result = eval_with_interpreter("6n * 7n");
    assert_eq!(result, JsValue::BigInt(42));
}

#[test]
fn interpreter_compares_bigints() {
    let result = eval_with_interpreter("1n < 2n");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn interpreter_supports_bigint_equality() {
    let result = eval_with_interpreter("1n === 1n");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn interpreter_rejects_mixed_bigint_arithmetic() {
    let result = eval_with_interpreter_result("1n + 1");
    assert!(matches!(result, Err(RuntimeError::TypeError(message)) if message.contains("cannot mix BigInt")));
}

#[test]
fn interpreter_rejects_unary_plus_on_bigint() {
    let result = eval_with_interpreter_result("+1n");
    assert!(matches!(result, Err(RuntimeError::TypeError(message)) if message.contains("cannot convert BigInt")));
}
