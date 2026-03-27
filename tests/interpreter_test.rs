use ai_agent::engine::{Interpreter, JsValue};
use ai_agent::engine::interpreter::RuntimeError;
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
    let error = eval_with_interpreter_result("null.foo;")
        .expect_err("member access should fail");

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
    let error = eval_with_interpreter_result("null.foo = 1;")
        .expect_err("member assignment should fail");

    assert!(matches!(error, RuntimeError::TypeError(_)));
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
