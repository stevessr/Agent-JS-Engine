use ai_agent::lexer::Lexer;
use ai_agent::parser::Parser;
use ai_agent::parser::ast::{
    AssignmentOperator, BinaryOperator, ClassElement, ExportDefaultKind, Expression,
    ImportSpecifier, Literal, ObjectKey, ObjectProperty, ObjectPropertyKind, Param, Statement,
    TemplatePart,
};

#[test]
fn parser_keeps_multiple_function_parameters() {
    let source = r#"
    function add(a, b) {
        return a + b;
    }
    "#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::FunctionDeclaration(func) => {
            assert_eq!(func.params.len(), 2);
            assert!(matches!(
                func.params[0],
                Param {
                    pattern: Expression::Identifier("a"),
                    is_rest: false
                }
            ));
            assert!(matches!(
                func.params[1],
                Param {
                    pattern: Expression::Identifier("b"),
                    is_rest: false
                }
            ));
        }
        other => panic!("expected function declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_array_literals_with_holes() {
    let source = "[1, , 3,]";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::ArrayExpression(elements)) => {
            assert_eq!(elements.len(), 3);
            assert!(matches!(
                elements[0],
                Some(Expression::Literal(Literal::Number(1.0)))
            ));
            assert!(elements[1].is_none());
            assert!(matches!(
                elements[2],
                Some(Expression::Literal(Literal::Number(3.0)))
            ));
        }
        other => panic!("expected array expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_object_literals() {
    let source = r#"({ foo: 1, "bar": 2, })"#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::ObjectExpression(properties)) => {
            assert_eq!(properties.len(), 2);
            assert!(matches!(&properties[0].key, ObjectKey::Identifier("foo")));
            assert!(matches!(
                &properties[0].value,
                Expression::Literal(Literal::Number(1.0))
            ));
            assert!(matches!(&properties[1].key, ObjectKey::String("bar")));
            assert!(matches!(
                &properties[1].value,
                Expression::Literal(Literal::Number(2.0))
            ));
        }
        other => panic!("expected object expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_with_statement() {
    let source = "with (scope) value;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::WithStatement(with_stmt) => {
            assert!(matches!(with_stmt.object, Expression::Identifier("scope")));
            assert!(matches!(
                with_stmt.body.as_ref(),
                Statement::ExpressionStatement(Expression::Identifier("value"))
            ));
        }
        other => panic!("expected with statement, got {other:?}"),
    }
}

#[test]
fn parser_parses_member_expression_chains() {
    let source = "obj.foo[0]";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::MemberExpression(outer)) => {
            assert!(outer.computed);
            assert!(matches!(
                outer.property,
                Expression::Literal(Literal::Number(0.0))
            ));
            match &outer.object {
                Expression::MemberExpression(inner) => {
                    assert!(!inner.computed);
                    assert!(matches!(inner.object, Expression::Identifier("obj")));
                    assert!(matches!(inner.property, Expression::Identifier("foo")));
                }
                other => panic!("expected inner member expression, got {other:?}"),
            }
        }
        other => panic!("expected member expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_member_assignment_with_dot_property() {
    let source = "obj.foo = 1";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::AssignmentExpression(assign)) => {
            assert!(matches!(assign.operator, AssignmentOperator::Assign));
            assert!(matches!(
                assign.right,
                Expression::Literal(Literal::Number(1.0))
            ));
            match &assign.left {
                Expression::MemberExpression(member) => {
                    assert!(!member.computed);
                    assert!(matches!(member.object, Expression::Identifier("obj")));
                    assert!(matches!(member.property, Expression::Identifier("foo")));
                }
                other => panic!("expected member expression left-hand side, got {other:?}"),
            }
        }
        other => panic!("expected assignment expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_member_assignment_with_computed_property() {
    let source = "arr[0] = 1";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::AssignmentExpression(assign)) => {
            assert!(matches!(assign.operator, AssignmentOperator::Assign));
            assert!(matches!(
                assign.right,
                Expression::Literal(Literal::Number(1.0))
            ));
            match &assign.left {
                Expression::MemberExpression(member) => {
                    assert!(member.computed);
                    assert!(matches!(member.object, Expression::Identifier("arr")));
                    assert!(matches!(
                        member.property,
                        Expression::Literal(Literal::Number(0.0))
                    ));
                }
                other => panic!("expected member expression left-hand side, got {other:?}"),
            }
        }
        other => panic!("expected assignment expression, got {other:?}"),
    }
}

#[test]
fn parser_preserves_plus_assign_operator() {
    let source = "x += 1";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::AssignmentExpression(assign)) => {
            assert!(matches!(assign.operator, AssignmentOperator::PlusAssign));
            assert!(matches!(assign.left, Expression::Identifier("x")));
            assert!(matches!(
                assign.right,
                Expression::Literal(Literal::Number(1.0))
            ));
        }
        other => panic!("expected assignment expression, got {other:?}"),
    }
}

#[test]
fn parser_preserves_member_compound_assignment_operator() {
    let source = "obj.foo *= 2";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::AssignmentExpression(assign)) => {
            assert!(matches!(
                assign.operator,
                AssignmentOperator::MultiplyAssign
            ));
            match &assign.left {
                Expression::MemberExpression(member) => {
                    assert!(!member.computed);
                    assert!(matches!(member.object, Expression::Identifier("obj")));
                    assert!(matches!(member.property, Expression::Identifier("foo")));
                }
                other => panic!("expected member expression left-hand side, got {other:?}"),
            }
            assert!(matches!(
                assign.right,
                Expression::Literal(Literal::Number(2.0))
            ));
        }
        other => panic!("expected assignment expression, got {other:?}"),
    }
}

#[test]
fn parser_preserves_power_assign_operator() {
    let source = "x **= 3";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::AssignmentExpression(assign)) => {
            assert!(matches!(assign.operator, AssignmentOperator::PowerAssign));
            assert!(matches!(assign.left, Expression::Identifier("x")));
            assert!(matches!(
                assign.right,
                Expression::Literal(Literal::Number(3.0))
            ));
        }
        other => panic!("expected assignment expression, got {other:?}"),
    }
}

#[test]
fn parser_rejects_invalid_assignment_target() {
    let source = "(1 + 2) = 3;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("invalid assignment target should fail");

    assert!(matches!(error, ai_agent::parser::ParseError::InvalidAssignmentTarget));
}

#[test]
fn parser_rejects_power_assignment_in_variable_declaration() {
    let source = "let x **= 2;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("power assignment in declaration should fail");

    assert!(format!("{error}").contains("Assign"));
}

#[test]
fn parser_preserves_logical_or_assign_operator() {
    let source = "x ||= 1";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::AssignmentExpression(assign)) => {
            assert!(matches!(assign.operator, AssignmentOperator::LogicOrAssign));
            assert!(matches!(assign.left, Expression::Identifier("x")));
            assert!(matches!(
                assign.right,
                Expression::Literal(Literal::Number(1.0))
            ));
        }
        other => panic!("expected assignment expression, got {other:?}"),
    }
}

#[test]
fn parser_preserves_logical_and_assign_on_member() {
    let source = "obj.foo &&= 2";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::AssignmentExpression(assign)) => {
            assert!(matches!(
                assign.operator,
                AssignmentOperator::LogicAndAssign
            ));
            match &assign.left {
                Expression::MemberExpression(member) => {
                    assert!(!member.computed);
                    assert!(matches!(member.object, Expression::Identifier("obj")));
                    assert!(matches!(member.property, Expression::Identifier("foo")));
                }
                other => panic!("expected member expression left-hand side, got {other:?}"),
            }
            assert!(matches!(
                assign.right,
                Expression::Literal(Literal::Number(2.0))
            ));
        }
        other => panic!("expected assignment expression, got {other:?}"),
    }
}

#[test]
fn parser_preserves_nullish_assign_on_computed_member() {
    let source = "arr[0] ??= 3";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::AssignmentExpression(assign)) => {
            assert!(matches!(assign.operator, AssignmentOperator::NullishAssign));
            match &assign.left {
                Expression::MemberExpression(member) => {
                    assert!(member.computed);
                    assert!(matches!(member.object, Expression::Identifier("arr")));
                    assert!(matches!(
                        member.property,
                        Expression::Literal(Literal::Number(0.0))
                    ));
                }
                other => panic!("expected member expression left-hand side, got {other:?}"),
            }
            assert!(matches!(
                assign.right,
                Expression::Literal(Literal::Number(3.0))
            ));
        }
        other => panic!("expected assignment expression, got {other:?}"),
    }
}

#[test]
fn parser_rejects_logical_assignment_in_variable_declaration() {
    let source = "let x ||= 1;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("logical assignment in declaration should fail");

    assert!(format!("{error}").contains("Assign"));
}

#[test]
fn parser_preserves_bitand_assign_operator() {
    let source = "x &= 1";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::AssignmentExpression(assign)) => {
            assert!(matches!(assign.operator, AssignmentOperator::BitAndAssign));
            assert!(matches!(assign.left, Expression::Identifier("x")));
            assert!(matches!(
                assign.right,
                Expression::Literal(Literal::Number(1.0))
            ));
        }
        other => panic!("expected assignment expression, got {other:?}"),
    }
}

#[test]
fn parser_preserves_shift_assign_on_member() {
    let source = "obj.foo <<= 2";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::AssignmentExpression(assign)) => {
            assert!(matches!(
                assign.operator,
                AssignmentOperator::ShiftLeftAssign
            ));
            match &assign.left {
                Expression::MemberExpression(member) => {
                    assert!(!member.computed);
                    assert!(matches!(member.object, Expression::Identifier("obj")));
                    assert!(matches!(member.property, Expression::Identifier("foo")));
                }
                other => panic!("expected member expression left-hand side, got {other:?}"),
            }
            assert!(matches!(
                assign.right,
                Expression::Literal(Literal::Number(2.0))
            ));
        }
        other => panic!("expected assignment expression, got {other:?}"),
    }
}

#[test]
fn parser_preserves_unsigned_shift_assign_on_computed_member() {
    let source = "arr[0] >>>= 1";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::AssignmentExpression(assign)) => {
            assert!(matches!(
                assign.operator,
                AssignmentOperator::UnsignedShiftRightAssign
            ));
            match &assign.left {
                Expression::MemberExpression(member) => {
                    assert!(member.computed);
                    assert!(matches!(member.object, Expression::Identifier("arr")));
                    assert!(matches!(
                        member.property,
                        Expression::Literal(Literal::Number(0.0))
                    ));
                }
                other => panic!("expected member expression left-hand side, got {other:?}"),
            }
            assert!(matches!(
                assign.right,
                Expression::Literal(Literal::Number(1.0))
            ));
        }
        other => panic!("expected assignment expression, got {other:?}"),
    }
}

#[test]
fn parser_rejects_bitwise_assignment_in_variable_declaration() {
    let source = "let x &= 1;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("bitwise assignment in declaration should fail");

    assert!(format!("{error}").contains("Assign"));
}

#[test]
fn parser_rejects_invalid_assignment_targets() {
    let source = "[value] += 2;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("invalid assignment target should fail");

    assert!(format!("{error}").contains("Invalid assignment target"));
}

#[test]
fn parser_rejects_invalid_update_targets() {
    let source = "++(value + 1);";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("invalid update target should fail");

    assert!(format!("{error}").contains("Invalid update target"));
}

#[test]
fn parser_rejects_const_without_initializer() {
    let source = "const value;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("const without initializer should fail");

    assert!(format!("{error}").contains("Missing initializer in const declaration"));
}

#[test]
fn parser_rejects_export_const_without_initializer() {
    let source = "export const value;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("export const without initializer should fail");

    assert!(format!("{error}").contains("Missing initializer in const declaration"));
}

#[test]
fn parser_rejects_non_terminal_array_rest_binding() {
    let source = "let [...rest, value] = items;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("non-terminal array rest binding should fail");

    assert!(format!("{error}").contains("Rest element must be last"));
}

#[test]
fn parser_rejects_trailing_comma_after_array_rest_binding() {
    let source = "let [...rest,] = items;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("array rest binding with trailing comma should fail");

    assert!(format!("{error}").contains("Rest element must be last"));
}

#[test]
fn parser_rejects_non_terminal_object_rest_binding() {
    let source = "let { ...rest, value } = obj;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("non-terminal object rest binding should fail");

    assert!(format!("{error}").contains("Rest property must be last"));
}

#[test]
fn parser_rejects_trailing_comma_after_object_rest_binding() {
    let source = "let { ...rest, } = obj;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("object rest binding with trailing comma should fail");

    assert!(format!("{error}").contains("Rest property must be last"));
}

#[test]
fn parser_parses_unary_bitnot_expression() {
    let source = "~x";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::UnaryExpression(unary)) => {
            assert!(matches!(
                unary.operator,
                ai_agent::parser::ast::UnaryOperator::BitNot
            ));
            assert!(matches!(unary.argument, Expression::Identifier("x")));
            assert!(unary.prefix);
        }
        other => panic!("expected unary expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_await_expression() {
    let source = "await value + 1";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::BinaryExpression(expr)) => {
            assert!(matches!(expr.operator, BinaryOperator::Plus));
            match &expr.left {
                Expression::AwaitExpression(argument) => {
                    assert!(matches!(argument.as_ref(), Expression::Identifier("value")));
                }
                other => panic!("expected await expression, got {other:?}"),
            }
            assert!(matches!(
                expr.right,
                Expression::Literal(Literal::Number(1.0))
            ));
        }
        other => panic!("expected binary expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_yield_expression() {
    let source = "yield value + 1";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::YieldExpression {
            argument: Some(argument),
            delegate: false,
        }) => match argument.as_ref() {
            Expression::BinaryExpression(expr) => {
                assert!(matches!(expr.operator, BinaryOperator::Plus));
                assert!(matches!(expr.left, Expression::Identifier("value")));
                assert!(matches!(
                    expr.right,
                    Expression::Literal(Literal::Number(1.0))
                ));
            }
            other => panic!("expected binary expression, got {other:?}"),
        },
        other => panic!("expected yield expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_yield_delegate_expression() {
    let source = "yield* values";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::YieldExpression {
            argument: Some(argument),
            delegate: true,
        }) => {
            assert!(matches!(
                argument.as_ref(),
                Expression::Identifier("values")
            ));
        }
        other => panic!("expected delegated yield expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_async_function_declaration() {
    let source = "async function load() { return 1; }";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::FunctionDeclaration(func) => {
            assert!(func.is_async);
            assert!(!func.is_generator);
            assert_eq!(func.id, Some("load"));
        }
        other => panic!("expected function declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_async_generator_function_declaration() {
    let source = "async function* load() { yield 1; }";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::FunctionDeclaration(func) => {
            assert!(func.is_async);
            assert!(func.is_generator);
            assert_eq!(func.id, Some("load"));
        }
        other => panic!("expected function declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_async_arrow_function() {
    let source = "async value => value + 1";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::ArrowFunctionExpression(func)) => {
            assert!(func.is_async);
            assert_eq!(func.params.len(), 1);
            assert!(matches!(
                func.params[0],
                Param {
                    pattern: Expression::Identifier("value"),
                    is_rest: false
                }
            ));
        }
        other => panic!("expected async arrow function, got {other:?}"),
    }
}

#[test]
fn parser_parses_async_class_method() {
    let source = r#"
    class Service {
        async load(value) { return value; }
    }
    "#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            let method = class_decl
                .body
                .iter()
                .find_map(|element| match element {
                    ClassElement::Method { key, value, .. }
                        if matches!(key, ObjectKey::Identifier("load")) =>
                    {
                        Some(value)
                    }
                    _ => None,
                })
                .expect("expected load method");
            assert!(method.is_async);
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_keyword_named_member_access() {
    let source = "obj.default";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::MemberExpression(member)) => {
            assert!(matches!(member.object, Expression::Identifier("obj")));
            assert!(matches!(member.property, Expression::Identifier("default")));
        }
        other => panic!("expected member expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_generator_object_method() {
    let source = r#"({ *items() { yield 1; } })"#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::ObjectExpression(properties)) => {
            assert_eq!(properties.len(), 1);
            match &properties[0].value {
                Expression::FunctionExpression(func) => assert!(func.is_generator),
                other => panic!("expected function expression, got {other:?}"),
            }
        }
        other => panic!("expected object expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_async_generator_class_method() {
    let source = r#"
    class Service {
        async *load() { yield 1; }
    }
    "#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            let method = class_decl
                .body
                .iter()
                .find_map(|element| match element {
                    ClassElement::Method { key, value, .. }
                        if matches!(key, ObjectKey::Identifier("load")) =>
                    {
                        Some(value)
                    }
                    _ => None,
                })
                .expect("expected load method");

            assert!(method.is_async);
            assert!(method.is_generator);
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_dynamic_import_call() {
    let source = r#"import("./dep.js")"#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::CallExpression(call)) => {
            assert!(matches!(call.callee, Expression::Identifier("import")));
            assert_eq!(call.arguments.len(), 1);
            assert!(matches!(
                call.arguments[0],
                Expression::Literal(Literal::String("./dep.js"))
            ));
        }
        other => panic!("expected call expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_import_declaration_with_default_and_named_specifiers() {
    let source = r#"import foo, { bar as baz, default as qux } from "./dep.js";"#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ImportDeclaration(import_decl) => {
            assert_eq!(import_decl.source, "./dep.js");
            assert_eq!(import_decl.specifiers.len(), 3);
            assert!(matches!(
                import_decl.specifiers[0],
                ImportSpecifier::Default("foo")
            ));
            assert!(matches!(
                import_decl.specifiers[1],
                ImportSpecifier::Named {
                    imported: "bar",
                    local: "baz"
                }
            ));
            assert!(matches!(
                import_decl.specifiers[2],
                ImportSpecifier::Named {
                    imported: "default",
                    local: "qux"
                }
            ));
        }
        other => panic!("expected import declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_import_declaration_with_string_named_specifier() {
    let source = r#"import { "default" as foo, bar } from "./dep.js";"#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ImportDeclaration(import_decl) => {
            assert_eq!(import_decl.source, "./dep.js");
            assert!(matches!(
                import_decl.specifiers.as_slice(),
                [
                    ImportSpecifier::Named {
                        imported: "default",
                        local: "foo"
                    },
                    ImportSpecifier::Named {
                        imported: "bar",
                        local: "bar"
                    }
                ]
            ));
        }
        other => panic!("expected import declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_export_default_expression() {
    let source = "export default 42;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExportDefaultDeclaration(export_decl) => match &export_decl.declaration {
            ExportDefaultKind::Expression(Expression::Literal(Literal::Number(42.0))) => {}
            other => panic!("expected default export expression, got {other:?}"),
        },
        other => panic!("expected export default declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_export_named_declaration() {
    let source = "export const value = 1;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExportNamedDeclaration(export_decl) => {
            assert!(export_decl.specifiers.is_empty());
            match export_decl.declaration.as_deref() {
                Some(Statement::VariableDeclaration(decl)) => {
                    assert_eq!(decl.declarations.len(), 1);
                    assert!(matches!(
                        decl.declarations[0].id,
                        Expression::Identifier("value")
                    ));
                }
                other => panic!("expected variable declaration, got {other:?}"),
            }
        }
        other => panic!("expected export named declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_export_named_specifiers_with_string_names() {
    let source = r#"export { foo as "bar", "baz" as qux };"#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExportNamedDeclaration(export_decl) => {
            assert!(export_decl.declaration.is_none());
            assert!(matches!(
                export_decl.specifiers.as_slice(),
                [
                    ai_agent::parser::ast::ExportSpecifier {
                        local: "foo",
                        exported: "bar"
                    },
                    ai_agent::parser::ast::ExportSpecifier {
                        local: "baz",
                        exported: "qux"
                    }
                ]
            ));
        }
        other => panic!("expected export named declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_export_all_as_namespace() {
    let source = r#"export * as ns from "./dep.js";"#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExportAllDeclaration(export_decl) => {
            assert_eq!(export_decl.exported, Some("ns"));
            assert_eq!(export_decl.source, "./dep.js");
        }
        other => panic!("expected export all declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_export_all_without_namespace() {
    let source = r#"export * from "./dep.js";"#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExportAllDeclaration(export_decl) => {
            assert_eq!(export_decl.exported, None);
            assert_eq!(export_decl.source, "./dep.js");
        }
        other => panic!("expected export all declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_array_destructuring_declaration() {
    let source = "let [first, , ...rest] = items;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::VariableDeclaration(decl) => {
            assert_eq!(decl.declarations.len(), 1);
            match &decl.declarations[0].id {
                Expression::ArrayExpression(elements) => {
                    assert!(matches!(
                        &elements[0],
                        Some(Expression::Identifier("first"))
                    ));
                    assert!(elements[1].is_none());
                    assert!(matches!(
                        &elements[2],
                        Some(Expression::SpreadElement(inner))
                            if matches!(inner.as_ref(), Expression::Identifier("rest"))
                    ));
                }
                other => panic!("expected array destructuring pattern, got {other:?}"),
            }
            assert!(matches!(
                decl.declarations[0].init,
                Some(Expression::Identifier("items"))
            ));
        }
        other => panic!("expected variable declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_object_destructuring_declaration() {
    let source = "let { foo, bar: baz = 1, ...rest } = obj;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::VariableDeclaration(decl) => match &decl.declarations[0].id {
            Expression::ObjectExpression(properties) => {
                assert_eq!(properties.len(), 3);
                assert!(matches!(properties[0].key, ObjectKey::Identifier("foo")));
                assert!(matches!(properties[0].value, Expression::Identifier("foo")));
                assert!(matches!(properties[1].key, ObjectKey::Identifier("bar")));
                assert!(matches!(
                    properties[1].value,
                    Expression::AssignmentExpression(_)
                ));
                assert!(matches!(properties[2].value, Expression::SpreadElement(_)));
            }
            other => panic!("expected object destructuring pattern, got {other:?}"),
        },
        other => panic!("expected variable declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_destructuring_function_parameters() {
    let source = r#"
    function load({ value } = source, [first, ...rest]) {
        return first;
    }
    "#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::FunctionDeclaration(func) => {
            assert_eq!(func.params.len(), 2);
            assert!(matches!(
                &func.params[0],
                Param {
                    pattern: Expression::AssignmentExpression(assign),
                    is_rest: false,
                } if matches!(
                    (&assign.left, &assign.right),
                    (
                        Expression::ObjectExpression(_),
                        Expression::Identifier("source")
                    )
                )
            ));
            assert!(matches!(
                &func.params[1],
                Param {
                    pattern: Expression::ArrayExpression(elements),
                    is_rest: false,
                } if matches!(
                    elements.as_slice(),
                    [
                        Some(Expression::Identifier("first")),
                        Some(Expression::SpreadElement(inner))
                    ] if matches!(inner.as_ref(), Expression::Identifier("rest"))
                )
            ));
        }
        other => panic!("expected function declaration, got {other:?}"),
    }
}

#[test]
fn parser_treats_debugger_as_empty_statement() {
    let source = "debugger;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    assert!(matches!(program.body[0], Statement::EmptyStatement));
}

#[test]
fn parser_respects_binary_operator_precedence() {
    let lexer = Lexer::new("1 + 2 * 3");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::BinaryExpression(expr)) => {
            assert!(matches!(expr.operator, BinaryOperator::Plus));
            assert!(matches!(
                expr.left,
                Expression::Literal(Literal::Number(1.0))
            ));
            match &expr.right {
                Expression::BinaryExpression(right) => {
                    assert!(matches!(right.operator, BinaryOperator::Multiply));
                    assert!(matches!(
                        right.left,
                        Expression::Literal(Literal::Number(2.0))
                    ));
                    assert!(matches!(
                        right.right,
                        Expression::Literal(Literal::Number(3.0))
                    ));
                }
                other => panic!("expected multiply expression, got {other:?}"),
            }
        }
        other => panic!("expected binary expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_class_method_default_parameter() {
    let source = r#"
    class Greeter {
        greet(name = "world") {
            return name;
        }
    }
    "#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            let method = class_decl
                .body
                .iter()
                .find_map(|element| match element {
                    ClassElement::Method { key, value, .. }
                        if matches!(key, ObjectKey::Identifier("greet")) =>
                    {
                        Some(value)
                    }
                    _ => None,
                })
                .expect("expected greet method");

            assert_eq!(method.params.len(), 1);
            match &method.params[0] {
                Param {
                    pattern: Expression::AssignmentExpression(assign),
                    is_rest: false,
                } if matches!(
                    (&assign.left, &assign.right),
                    (
                        Expression::Identifier("name"),
                        Expression::Literal(Literal::String("world"))
                    )
                ) => {}
                other => panic!("expected default parameter, got {other:?}"),
            }
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_array_spread_expression() {
    let lexer = Lexer::new("[1, ...rest, 3]");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::ArrayExpression(elements)) => {
            assert_eq!(elements.len(), 3);
            assert!(matches!(
                elements[0],
                Some(Expression::Literal(Literal::Number(1.0)))
            ));
            assert!(matches!(
                &elements[1],
                Some(Expression::SpreadElement(inner)) if matches!(inner.as_ref(), Expression::Identifier("rest"))
            ));
            assert!(matches!(
                elements[2],
                Some(Expression::Literal(Literal::Number(3.0)))
            ));
        }
        other => panic!("expected array expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_for_in_statement() {
    let lexer = Lexer::new("for (let key in obj) key;");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ForInStatement(stmt) => {
            assert!(matches!(stmt.right, Expression::Identifier("obj")));
            match stmt.left.as_ref() {
                Statement::VariableDeclaration(decl) => {
                    assert_eq!(decl.declarations.len(), 1);
                    assert!(matches!(
                        decl.declarations[0].id,
                        Expression::Identifier("key")
                    ));
                }
                other => panic!("expected variable declaration, got {other:?}"),
            }
        }
        other => panic!("expected for-in statement, got {other:?}"),
    }
}

#[test]
fn parser_rejects_const_initializer_in_for_statement() {
    let lexer = Lexer::new("for (const value;;) value;");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("const initializer in plain for loop should fail");

    assert!(format!("{error}").contains("Missing initializer in const declaration"));
}

#[test]
fn parser_rejects_initializer_in_for_of_declaration() {
    let lexer = Lexer::new("for (const value = 1 of list) value;");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("for-of declaration initializer should fail");

    assert!(format!("{error}").contains("for-in/of declarations cannot have initializers"));
}

#[test]
fn parser_rejects_initializer_in_for_in_declaration() {
    let lexer = Lexer::new("for (const key = 1 in obj) key;");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("for-in declaration initializer should fail");

    assert!(format!("{error}").contains("for-in/of declarations cannot have initializers"));
}

#[test]
fn parser_rejects_invalid_for_of_binding_target() {
    let lexer = Lexer::new("for (value + 1 of list) value;");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("invalid for-of binding target should fail");

    assert!(format!("{error}").contains("Invalid for-in/of binding"));
}

#[test]
fn parser_parses_for_of_statement() {
    let lexer = Lexer::new("for (item of list) item;");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ForOfStatement(stmt) => {
            assert!(!stmt.is_await);
            assert!(matches!(stmt.right, Expression::Identifier("list")));
            match stmt.left.as_ref() {
                Statement::ExpressionStatement(Expression::Identifier("item")) => {}
                other => panic!("expected identifier initializer, got {other:?}"),
            }
        }
        other => panic!("expected for-of statement, got {other:?}"),
    }
}

#[test]
fn parser_parses_for_await_of_statement() {
    let lexer = Lexer::new("for await (const item of list) item;");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ForOfStatement(stmt) => {
            assert!(stmt.is_await);
            assert!(matches!(stmt.right, Expression::Identifier("list")));
        }
        other => panic!("expected for-await-of statement, got {other:?}"),
    }
}

#[test]
fn parser_rejects_for_await_in_statement() {
    let lexer = Lexer::new("for await (const item in list) item;");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("for-await-in should fail");

    assert!(format!("{error}").contains("Expected of"));
}

#[test]
fn parser_parses_for_of_statement_with_destructuring_binding() {
    let lexer = Lexer::new("for (let { value, ...rest } of list) value;");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ForOfStatement(stmt) => match stmt.left.as_ref() {
            Statement::VariableDeclaration(decl) => {
                assert_eq!(decl.declarations.len(), 1);
                assert!(matches!(
                    &decl.declarations[0].id,
                    Expression::ObjectExpression(properties)
                        if matches!(
                            properties.as_slice(),
                            [
                                ObjectProperty {
                                    key: ObjectKey::Identifier("value"),
                                    value: Expression::Identifier("value"),
                                    ..
                                },
                                ObjectProperty {
                                    value: Expression::SpreadElement(_),
                                    ..
                                }
                            ]
                        )
                ));
            }
            other => panic!("expected variable declaration, got {other:?}"),
        },
        other => panic!("expected for-of statement, got {other:?}"),
    }
}

#[test]
fn parser_parses_catch_destructuring_parameter() {
    let lexer = Lexer::new("try { throw err; } catch ({ message, ...rest }) { message; }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::TryStatement(stmt) => {
            let handler = stmt.handler.as_ref().expect("expected catch handler");
            assert!(matches!(
                &handler.param,
                Some(Expression::ObjectExpression(properties))
                    if matches!(
                        properties.as_slice(),
                        [
                            ObjectProperty {
                                key: ObjectKey::Identifier("message"),
                                value: Expression::Identifier("message"),
                                ..
                            },
                            ObjectProperty {
                                value: Expression::SpreadElement(_),
                                ..
                            }
                        ]
                    )
            ));
        }
        other => panic!("expected try statement, got {other:?}"),
    }
}

#[test]
fn parser_parses_switch_statement() {
    let lexer = Lexer::new("switch (x) { case 1: y; break; default: z; }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::SwitchStatement(stmt) => {
            assert!(matches!(stmt.discriminant, Expression::Identifier("x")));
            assert_eq!(stmt.cases.len(), 2);
            assert!(matches!(
                stmt.cases[0].test,
                Some(Expression::Literal(Literal::Number(1.0)))
            ));
            assert!(matches!(
                stmt.cases[0].consequent[1],
                Statement::BreakStatement(None)
            ));
            assert!(stmt.cases[1].test.is_none());
        }
        other => panic!("expected switch statement, got {other:?}"),
    }
}

#[test]
fn parser_parses_do_while_statement() {
    let lexer = Lexer::new("do x; while (y);");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::DoWhileStatement(stmt) => {
            assert!(matches!(stmt.test, Expression::Identifier("y")));
            assert!(matches!(
                stmt.body.as_ref(),
                Statement::ExpressionStatement(Expression::Identifier("x"))
            ));
        }
        other => panic!("expected do-while statement, got {other:?}"),
    }
}

#[test]
fn parser_parses_labeled_break_statement() {
    let lexer = Lexer::new("outer: break outer;");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::LabeledStatement(stmt) => {
            assert_eq!(stmt.label, "outer");
            assert!(matches!(
                stmt.body.as_ref(),
                Statement::BreakStatement(Some("outer"))
            ));
        }
        other => panic!("expected labeled statement, got {other:?}"),
    }
}

#[test]
fn parser_parses_class_declaration_with_constructor_and_method() {
    let lexer = Lexer::new("class Foo { constructor() {} bar() {} }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            assert_eq!(class_decl.id, Some("Foo"));
            assert!(class_decl.super_class.is_none());
            assert_eq!(class_decl.body.len(), 2);
            assert!(matches!(
                class_decl.body[0],
                ClassElement::Constructor {
                    is_default: false,
                    ..
                }
            ));
            assert!(matches!(
                &class_decl.body[1],
                ClassElement::Method {
                    key: ObjectKey::Identifier("bar"),
                    ..
                }
            ));
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_class_extends_and_super_call() {
    let lexer = Lexer::new("class Foo extends Bar { constructor() { super(); } }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            assert!(matches!(
                class_decl.super_class,
                Some(Expression::Identifier("Bar"))
            ));
            match &class_decl.body[0] {
                ClassElement::Constructor {
                    function,
                    is_default,
                } => {
                    assert!(!is_default);
                    assert!(matches!(
                        &function.body.body[0],
                        Statement::ExpressionStatement(Expression::CallExpression(call))
                            if matches!(call.callee, Expression::SuperExpression)
                    ));
                }
                other => panic!("expected constructor, got {other:?}"),
            }
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_super_method_call() {
    let lexer = Lexer::new("class Foo extends Bar { bar() { super.bar(); } }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            let method = class_decl
                .body
                .iter()
                .find_map(|element| match element {
                    ClassElement::Method {
                        key: ObjectKey::Identifier("bar"),
                        value,
                        ..
                    } => Some(value),
                    _ => None,
                })
                .expect("expected bar method");
            assert!(matches!(
                &method.body.body[0],
                Statement::ExpressionStatement(Expression::CallExpression(call))
                    if matches!(
                        call.callee,
                        Expression::MemberExpression(ref member)
                            if matches!(member.object, Expression::SuperExpression)
                                && matches!(member.property, Expression::Identifier("bar"))
                    )
            ));
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_inserts_default_derived_constructor() {
    let lexer = Lexer::new("class Foo extends Bar {}");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => match &class_decl.body[0] {
            ClassElement::Constructor {
                function,
                is_default,
            } => {
                assert!(*is_default);
                assert!(matches!(
                    function.params.as_slice(),
                    [Param {
                        pattern: Expression::Identifier("args"),
                        is_rest: true
                    }]
                ));
                assert!(matches!(
                    &function.body.body[0],
                    Statement::ExpressionStatement(Expression::CallExpression(call))
                        if matches!(call.callee, Expression::SuperExpression)
                ));
            }
            other => panic!("expected default constructor, got {other:?}"),
        },
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_super_computed_method_call() {
    let lexer = Lexer::new("class Foo extends Bar { bar() { super[key](); } }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            let method = class_decl
                .body
                .iter()
                .find_map(|element| match element {
                    ClassElement::Method {
                        key: ObjectKey::Identifier("bar"),
                        value,
                        ..
                    } => Some(value),
                    _ => None,
                })
                .expect("expected bar method");
            assert!(matches!(
                &method.body.body[0],
                Statement::ExpressionStatement(Expression::CallExpression(call))
                    if matches!(
                        call.callee,
                        Expression::MemberExpression(ref member)
                            if matches!(member.object, Expression::SuperExpression)
                                && member.computed
                                && matches!(member.property, Expression::Identifier("key"))
                    )
            ));
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_static_method() {
    let lexer = Lexer::new("class Foo { static bar() {} }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            assert!(matches!(
                &class_decl.body[0],
                ClassElement::Method {
                    key: ObjectKey::Identifier("bar"),
                    is_static: true,
                    ..
                }
            ));
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_static_field() {
    let lexer = Lexer::new("class Foo { static value = 1; }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            assert!(matches!(
                &class_decl.body[0],
                ClassElement::Field {
                    key: ObjectKey::Identifier("value"),
                    initializer: Some(Expression::Literal(Literal::Number(1.0))),
                    is_static: true,
                }
            ));
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_instance_field() {
    let lexer = Lexer::new("class Foo { value = 1; }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            assert!(matches!(
                &class_decl.body[0],
                ClassElement::Field {
                    key: ObjectKey::Identifier("value"),
                    initializer: Some(Expression::Literal(Literal::Number(1.0))),
                    is_static: false,
                }
            ));
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_computed_static_method() {
    let lexer = Lexer::new("class Foo { static [key]() {} }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            assert!(matches!(
                &class_decl.body[0],
                ClassElement::Method {
                    key: ObjectKey::Computed(expr),
                    is_static: true,
                    ..
                } if matches!(expr.as_ref(), Expression::Identifier("key"))
            ));
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_object_getter_and_setter() {
    let lexer = Lexer::new("({ get value() {}, set value(v) {} })");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::ObjectExpression(properties)) => {
            assert!(matches!(properties[0].kind, ObjectPropertyKind::Getter(_)));
            assert!(matches!(properties[1].kind, ObjectPropertyKind::Setter(_)));
        }
        other => panic!("expected object expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_class_getter_and_setter() {
    let lexer = Lexer::new("class Foo { get value() {} set value(v) {} }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            assert!(matches!(class_decl.body[0], ClassElement::Getter { .. }));
            assert!(matches!(class_decl.body[1], ClassElement::Setter { .. }));
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_static_getter_and_setter() {
    let lexer = Lexer::new("class Foo { static get value() {} static set value(v) {} }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            assert!(matches!(
                class_decl.body[0],
                ClassElement::Getter {
                    is_static: true,
                    ..
                }
            ));
            assert!(matches!(
                class_decl.body[1],
                ClassElement::Setter {
                    is_static: true,
                    ..
                }
            ));
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_optional_member_expression() {
    let lexer = Lexer::new("obj?.foo");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::MemberExpression(member)) => {
            assert!(member.optional);
            assert!(!member.computed);
            assert!(matches!(member.object, Expression::Identifier("obj")));
            assert!(matches!(member.property, Expression::Identifier("foo")));
        }
        other => panic!("expected optional member expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_optional_computed_member_expression() {
    let lexer = Lexer::new("obj?.[key]");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::MemberExpression(member)) => {
            assert!(member.optional);
            assert!(member.computed);
            assert!(matches!(member.object, Expression::Identifier("obj")));
            assert!(matches!(member.property, Expression::Identifier("key")));
        }
        other => panic!("expected optional computed member expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_optional_call_expression() {
    let lexer = Lexer::new("fnRef?.()");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::CallExpression(call)) => {
            assert!(call.optional);
            assert!(matches!(call.callee, Expression::Identifier("fnRef")));
            assert!(call.arguments.is_empty());
        }
        other => panic!("expected optional call expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_regex_literal() {
    let lexer = Lexer::new("/foo/");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::Literal(Literal::RegExp("foo", ""))) => {}
        other => panic!("expected regexp literal, got {other:?}"),
    }
}

#[test]
fn parser_parses_regex_literal_with_flags() {
    let lexer = Lexer::new("/foo/gi");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::Literal(Literal::RegExp("foo", "gi"))) => {}
        other => panic!("expected regexp literal with flags, got {other:?}"),
    }
}

#[test]
fn parser_parses_template_literal_with_expression() {
    let lexer = Lexer::new("`a${value}b`");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::TemplateLiteral(parts)) => {
            assert_eq!(parts.len(), 3);
            assert!(matches!(parts[0], TemplatePart::String("a")));
            assert!(matches!(
                parts[1],
                TemplatePart::Expr(Expression::Identifier("value"))
            ));
            assert!(matches!(parts[2], TemplatePart::String("b")));
        }
        other => panic!("expected template literal, got {other:?}"),
    }
}

#[test]
fn parser_parses_template_literal_with_empty_segments() {
    let lexer = Lexer::new("`${value}`");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::TemplateLiteral(parts)) => {
            assert_eq!(parts.len(), 3);
            assert!(matches!(parts[0], TemplatePart::String("")));
            assert!(matches!(
                parts[1],
                TemplatePart::Expr(Expression::Identifier("value"))
            ));
            assert!(matches!(parts[2], TemplatePart::String("")));
        }
        other => panic!("expected template literal, got {other:?}"),
    }
}

#[test]
fn parser_parses_tagged_template_expression() {
    let lexer = Lexer::new("tag`a${value}b`");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::TaggedTemplateExpression(tag, parts)) => {
            assert!(matches!(tag.as_ref(), Expression::Identifier("tag")));
            assert_eq!(parts.len(), 3);
            assert!(matches!(parts[0], TemplatePart::String("a")));
            assert!(matches!(
                parts[1],
                TemplatePart::Expr(Expression::Identifier("value"))
            ));
            assert!(matches!(parts[2], TemplatePart::String("b")));
        }
        other => panic!("expected tagged template expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_private_class_elements_and_brand_checks() {
    let source = r#"
    class Foo {
        #x = 1;
        get #value() { return this.#x; }
        set #value(v) { this.#x = v; }
        #m() { return this.#x; }
        static #count = 2;
    }

    #x in foo;
    this.#x;
    "#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            assert!(matches!(
                class_decl.body[0],
                ClassElement::Field {
                    key: ObjectKey::PrivateIdentifier("x"),
                    is_static: false,
                    ..
                }
            ));
            assert!(matches!(
                class_decl.body[1],
                ClassElement::Getter {
                    key: ObjectKey::PrivateIdentifier("value"),
                    is_static: false,
                    ..
                }
            ));
            assert!(matches!(
                class_decl.body[2],
                ClassElement::Setter {
                    key: ObjectKey::PrivateIdentifier("value"),
                    is_static: false,
                    ..
                }
            ));
            assert!(matches!(
                class_decl.body[3],
                ClassElement::Method {
                    key: ObjectKey::PrivateIdentifier("m"),
                    is_static: false,
                    ..
                }
            ));
            assert!(matches!(
                class_decl.body[4],
                ClassElement::Field {
                    key: ObjectKey::PrivateIdentifier("count"),
                    is_static: true,
                    ..
                }
            ));
        }
        other => panic!("expected class declaration, got {other:?}"),
    }

    match &program.body[1] {
        Statement::ExpressionStatement(Expression::BinaryExpression(expr)) => {
            assert!(matches!(expr.left, Expression::PrivateIdentifier("x")));
            assert!(matches!(expr.operator, BinaryOperator::In));
            assert!(matches!(expr.right, Expression::Identifier("foo")));
        }
        other => panic!("expected private brand check, got {other:?}"),
    }

    match &program.body[2] {
        Statement::ExpressionStatement(Expression::MemberExpression(expr)) => {
            assert!(matches!(expr.object, Expression::ThisExpression));
            assert!(matches!(expr.property, Expression::PrivateIdentifier("x")));
            assert!(!expr.computed);
        }
        other => panic!("expected private member expression, got {other:?}"),
    }
}

#[test]
fn parser_rejects_optional_private_member_chains() {
    let lexer = Lexer::new("value?.#secret;");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("optional private member chain should fail");

    assert!(matches!(
        error,
        ai_agent::parser::ParseError::InvalidPrivateIdentifierUsage(_)
    ));
}


#[test]
fn parser_parses_super_assignment_targets() {
    let lexer = Lexer::new("class Foo extends Bar { write() { super.value = 1; super[key] ??= 2; } }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            let method = class_decl
                .body
                .iter()
                .find_map(|element| match element {
                    ClassElement::Method {
                        key: ObjectKey::Identifier("write"),
                        value,
                        ..
                    } => Some(value),
                    _ => None,
                })
                .expect("expected write method");

            assert!(matches!(
                &method.body.body[0],
                Statement::ExpressionStatement(Expression::AssignmentExpression(assign))
                    if matches!(assign.operator, AssignmentOperator::Assign)
                        && matches!(
                            assign.left,
                            Expression::MemberExpression(ref member)
                                if matches!(member.object, Expression::SuperExpression)
                                    && !member.computed
                                    && matches!(member.property, Expression::Identifier("value"))
                        )
            ));

            assert!(matches!(
                &method.body.body[1],
                Statement::ExpressionStatement(Expression::AssignmentExpression(assign))
                    if matches!(assign.operator, AssignmentOperator::NullishAssign)
                        && matches!(
                            assign.left,
                            Expression::MemberExpression(ref member)
                                if matches!(member.object, Expression::SuperExpression)
                                    && member.computed
                                    && matches!(member.property, Expression::Identifier("key"))
                        )
            ));
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_parses_private_assignment_targets() {
    let lexer = Lexer::new("class Foo { write() { this.#x = 1; this.#x ||= 2; } }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ClassDeclaration(class_decl) => {
            let method = class_decl
                .body
                .iter()
                .find_map(|element| match element {
                    ClassElement::Method {
                        key: ObjectKey::Identifier("write"),
                        value,
                        ..
                    } => Some(value),
                    _ => None,
                })
                .expect("expected write method");

            assert!(matches!(
                &method.body.body[0],
                Statement::ExpressionStatement(Expression::AssignmentExpression(assign))
                    if matches!(assign.operator, AssignmentOperator::Assign)
                        && matches!(
                            assign.left,
                            Expression::MemberExpression(ref member)
                                if matches!(member.object, Expression::ThisExpression)
                                    && !member.computed
                                    && matches!(member.property, Expression::PrivateIdentifier("x"))
                        )
            ));

            assert!(matches!(
                &method.body.body[1],
                Statement::ExpressionStatement(Expression::AssignmentExpression(assign))
                    if matches!(assign.operator, AssignmentOperator::LogicOrAssign)
                        && matches!(
                            assign.left,
                            Expression::MemberExpression(ref member)
                                if matches!(member.object, Expression::ThisExpression)
                                    && !member.computed
                                    && matches!(member.property, Expression::PrivateIdentifier("x"))
                        )
            ));
        }
        other => panic!("expected class declaration, got {other:?}"),
    }
}

#[test]
fn parser_rejects_super_as_assignment_target() {
    let lexer = Lexer::new("class Foo extends Bar { write() { super ||= 1; } }");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("super logical assignment target should fail");

    assert!(matches!(error, ai_agent::parser::ParseError::InvalidAssignmentTarget));
}
