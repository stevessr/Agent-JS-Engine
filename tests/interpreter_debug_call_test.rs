use ai_agent::lexer::Lexer;
use ai_agent::parser::Parser;
use ai_agent::parser::ast::{Expression, Literal, Statement};

#[test]
fn debug_call_expression_keeps_two_arguments() {
    let source = r#"
    function add(a, b) {
        return a + b;
    }

    add(20, 22);
    "#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).unwrap();
    let program = parser.parse_program().unwrap();

    match &program.body[1] {
        Statement::ExpressionStatement(Expression::CallExpression(call)) => {
            assert_eq!(call.arguments.len(), 2);
            match &call.arguments[0] {
                Expression::Literal(Literal::Number(n)) => assert_eq!(*n, 20.0),
                other => panic!("unexpected first arg: {other:?}"),
            }
            match &call.arguments[1] {
                Expression::Literal(Literal::Number(n)) => assert_eq!(*n, 22.0),
                other => panic!("unexpected second arg: {other:?}"),
            }
        }
        other => panic!("unexpected statement: {other:?}"),
    }
}
