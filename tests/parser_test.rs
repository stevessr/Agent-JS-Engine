use ai_agent::lexer::Lexer;
use ai_agent::parser::Parser;
use ai_agent::parser::ast::{
    AssignmentOperator, BinaryOperator, ClassElement, Expression, Literal, ObjectKey,
    ObjectPropertyKind, Param, Statement, TemplatePart,
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
            assert!(matches!(func.params[0], Param::Simple("a")));
            assert!(matches!(func.params[1], Param::Simple("b")));
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
                    assert_eq!(decl.declarations[0].id, "key");
                }
                other => panic!("expected variable declaration, got {other:?}"),
            }
        }
        other => panic!("expected for-in statement, got {other:?}"),
    }
}

#[test]
fn parser_parses_for_of_statement() {
    let lexer = Lexer::new("for (item of list) item;");
    let mut parser = Parser::new(lexer).expect("parser should initialize");
    let program = parser.parse_program().expect("program should parse");

    match &program.body[0] {
        Statement::ForOfStatement(stmt) => {
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
                assert!(matches!(function.params.as_slice(), [Param::Rest("args")]));
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
            assert!(matches!(parts[1], TemplatePart::Expr(Expression::Identifier("value"))));
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
            assert!(matches!(parts[1], TemplatePart::Expr(Expression::Identifier("value"))));
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
            assert!(matches!(parts[1], TemplatePart::Expr(Expression::Identifier("value"))));
            assert!(matches!(parts[2], TemplatePart::String("b")));
        }
        other => panic!("expected tagged template expression, got {other:?}"),
    }
}
