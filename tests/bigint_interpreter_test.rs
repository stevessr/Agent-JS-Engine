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

fn assert_type_error(result: Result<JsValue, RuntimeError>, expected: &str) {
    match result {
        Err(RuntimeError::TypeError(message)) => {
            assert!(
                message.contains(expected),
                "expected TypeError containing {expected:?}, got {message:?}"
            );
        }
        other => panic!("expected TypeError containing {expected:?}, got {other:?}"),
    }
}

fn assert_range_error(result: Result<JsValue, RuntimeError>, expected: &str) {
    match result {
        Err(RuntimeError::RangeError(message)) => {
            assert!(
                message.contains(expected),
                "expected RangeError containing {expected:?}, got {message:?}"
            );
        }
        other => panic!("expected RangeError containing {expected:?}, got {other:?}"),
    }
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
fn interpreter_divides_bigints() {
    let result = eval_with_interpreter("8n / 2n");
    assert_eq!(result, JsValue::BigInt(4));
}

#[test]
fn interpreter_divides_bigints_with_truncation() {
    let result = eval_with_interpreter("7n / 2n");
    assert_eq!(result, JsValue::BigInt(3));
}

#[test]
fn interpreter_rejects_bigint_division_by_zero() {
    let result = eval_with_interpreter_result("1n / 0n");
    assert_range_error(result, "Division by zero");
}

#[test]
fn interpreter_divides_bigint_assign() {
    let result = eval_with_interpreter("let x = 8n; x /= 2n; x");
    assert_eq!(result, JsValue::BigInt(4));
}

#[test]
fn interpreter_rejects_bigint_divide_assign_by_zero() {
    let result = eval_with_interpreter_result("let x = 1n; x /= 0n; x");
    assert_range_error(result, "Division by zero");
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
    assert_type_error(result, "cannot mix BigInt");
}

#[test]
fn interpreter_rejects_unary_plus_on_bigint() {
    let result = eval_with_interpreter_result("+1n");
    assert_type_error(result, "cannot convert BigInt");
}

#[test]
fn interpreter_rejects_bigint_remainder() {
    let result = eval_with_interpreter_result("1n % 1n");
    assert_type_error(result, "BigInt remainder is not supported yet");
}

#[test]
fn interpreter_rejects_bigint_exponentiation() {
    let result = eval_with_interpreter_result("1n ** 1n");
    assert_type_error(result, "BigInt exponentiation is not supported yet");
}

#[test]
fn interpreter_rejects_bigint_bitwise_and() {
    let result = eval_with_interpreter_result("1n & 1n");
    assert_type_error(result, "BigInt bitwise operations are not supported yet");
}

#[test]
fn interpreter_rejects_bigint_bitwise_or() {
    let result = eval_with_interpreter_result("1n | 1n");
    assert_type_error(result, "BigInt bitwise operations are not supported yet");
}

#[test]
fn interpreter_rejects_bigint_bitwise_xor() {
    let result = eval_with_interpreter_result("1n ^ 1n");
    assert_type_error(result, "BigInt bitwise operations are not supported yet");
}

#[test]
fn interpreter_rejects_bigint_left_shift() {
    let result = eval_with_interpreter_result("1n << 1n");
    assert_type_error(result, "BigInt shift operations are not supported yet");
}

#[test]
fn interpreter_rejects_bigint_right_shift() {
    let result = eval_with_interpreter_result("1n >> 1n");
    assert_type_error(result, "BigInt shift operations are not supported yet");
}

#[test]
fn interpreter_rejects_bigint_unsigned_right_shift() {
    let result = eval_with_interpreter_result("1n >>> 1n");
    assert_type_error(result, "BigInt shift operations are not supported yet");
}

#[test]
fn interpreter_rejects_bigint_remainder_assign() {
    let result = eval_with_interpreter_result("let x = 1n; x %= 1n; x");
    assert_type_error(result, "BigInt remainder is not supported yet");
}

#[test]
fn interpreter_rejects_bigint_power_assign() {
    let result = eval_with_interpreter_result("let x = 1n; x **= 1n; x");
    assert_type_error(result, "BigInt exponentiation is not supported yet");
}

#[test]
fn interpreter_rejects_bigint_bitand_assign() {
    let result = eval_with_interpreter_result("let x = 1n; x &= 1n; x");
    assert_type_error(result, "BigInt bitwise operations are not supported yet");
}

#[test]
fn interpreter_rejects_bigint_bitor_assign() {
    let result = eval_with_interpreter_result("let x = 1n; x |= 1n; x");
    assert_type_error(result, "BigInt bitwise operations are not supported yet");
}

#[test]
fn interpreter_rejects_bigint_bitxor_assign() {
    let result = eval_with_interpreter_result("let x = 1n; x ^= 1n; x");
    assert_type_error(result, "BigInt bitwise operations are not supported yet");
}

#[test]
fn interpreter_rejects_bigint_left_shift_assign() {
    let result = eval_with_interpreter_result("let x = 1n; x <<= 1n; x");
    assert_type_error(result, "BigInt shift operations are not supported yet");
}

#[test]
fn interpreter_rejects_bigint_right_shift_assign() {
    let result = eval_with_interpreter_result("let x = 1n; x >>= 1n; x");
    assert_type_error(result, "BigInt shift operations are not supported yet");
}

#[test]
fn interpreter_rejects_bigint_unsigned_right_shift_assign() {
    let result = eval_with_interpreter_result("let x = 1n; x >>>= 1n; x");
    assert_type_error(result, "BigInt shift operations are not supported yet");
}
