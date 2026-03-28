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
fn interpreter_short_circuits_logical_assignment_rhs() {
    let result = eval_with_interpreter(
        r#"
        let x = 1;
        let y = 0;
        x ||= (y = 1);
        y;
        "#,
    );

    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn interpreter_evaluates_bitwise_binary_and() {
    let result = eval_with_interpreter("5 & 3;");
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
