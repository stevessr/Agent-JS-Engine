use ai_agent::lexer::Lexer;
use ai_agent::parser::Parser;
use ai_agent::parser::ast::{AssignmentOperator, Expression, Literal, ObjectKey, Statement};

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
            assert_eq!(func.params, vec!["a", "b"]);
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
            assert!(matches!(elements[0], Some(Expression::Literal(Literal::Number(1.0)))));
            assert!(elements[1].is_none());
            assert!(matches!(elements[2], Some(Expression::Literal(Literal::Number(3.0)))));
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
            assert!(matches!(
                &properties[0],
                (ObjectKey::Identifier("foo"), Expression::Literal(Literal::Number(1.0)))
            ));
            assert!(matches!(
                &properties[1],
                (ObjectKey::String("bar"), Expression::Literal(Literal::Number(2.0)))
            ));
        }
        other => panic!("expected object expression, got {other:?}"),
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
            assert!(matches!(outer.property, Expression::Literal(Literal::Number(0.0))));
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
            assert!(matches!(assign.right, Expression::Literal(Literal::Number(1.0))));
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
            assert!(matches!(assign.right, Expression::Literal(Literal::Number(1.0))));
            match &assign.left {
                Expression::MemberExpression(member) => {
                    assert!(member.computed);
                    assert!(matches!(member.object, Expression::Identifier("arr")));
                    assert!(matches!(member.property, Expression::Literal(Literal::Number(0.0))));
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
            assert!(matches!(assign.right, Expression::Literal(Literal::Number(1.0))));
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
            assert!(matches!(assign.operator, AssignmentOperator::MultiplyAssign));
            match &assign.left {
                Expression::MemberExpression(member) => {
                    assert!(!member.computed);
                    assert!(matches!(member.object, Expression::Identifier("obj")));
                    assert!(matches!(member.property, Expression::Identifier("foo")));
                }
                other => panic!("expected member expression left-hand side, got {other:?}"),
            }
            assert!(matches!(assign.right, Expression::Literal(Literal::Number(2.0))));
        }
        other => panic!("expected assignment expression, got {other:?}"),
    }
}

#[test]
fn parser_rejects_compound_assignment_in_variable_declaration() {
    let source = "let x += 1;";

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let error = parser
        .parse_program()
        .expect_err("compound assignment in declaration should fail");

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
            assert!(matches!(assign.right, Expression::Literal(Literal::Number(1.0))));
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
            assert!(matches!(assign.operator, AssignmentOperator::LogicAndAssign));
            match &assign.left {
                Expression::MemberExpression(member) => {
                    assert!(!member.computed);
                    assert!(matches!(member.object, Expression::Identifier("obj")));
                    assert!(matches!(member.property, Expression::Identifier("foo")));
                }
                other => panic!("expected member expression left-hand side, got {other:?}"),
            }
            assert!(matches!(assign.right, Expression::Literal(Literal::Number(2.0))));
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
                    assert!(matches!(member.property, Expression::Literal(Literal::Number(0.0))));
                }
                other => panic!("expected member expression left-hand side, got {other:?}"),
            }
            assert!(matches!(assign.right, Expression::Literal(Literal::Number(3.0))));
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
            assert!(matches!(assign.right, Expression::Literal(Literal::Number(1.0))));
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
            assert!(matches!(assign.operator, AssignmentOperator::ShiftLeftAssign));
            match &assign.left {
                Expression::MemberExpression(member) => {
                    assert!(!member.computed);
                    assert!(matches!(member.object, Expression::Identifier("obj")));
                    assert!(matches!(member.property, Expression::Identifier("foo")));
                }
                other => panic!("expected member expression left-hand side, got {other:?}"),
            }
            assert!(matches!(assign.right, Expression::Literal(Literal::Number(2.0))));
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
            assert!(matches!(assign.operator, AssignmentOperator::UnsignedShiftRightAssign));
            match &assign.left {
                Expression::MemberExpression(member) => {
                    assert!(member.computed);
                    assert!(matches!(member.object, Expression::Identifier("arr")));
                    assert!(matches!(member.property, Expression::Literal(Literal::Number(0.0))));
                }
                other => panic!("expected member expression left-hand side, got {other:?}"),
            }
            assert!(matches!(assign.right, Expression::Literal(Literal::Number(1.0))));
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
