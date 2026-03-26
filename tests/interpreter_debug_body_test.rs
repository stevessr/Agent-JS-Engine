use ai_agent::lexer::Lexer;
use ai_agent::parser::Parser;
use ai_agent::parser::ast::{Expression, Statement};

#[test]
fn debug_function_body_returns_second_param_identifier() {
    let source = r#"
    function add(a, b) {
        return b;
    }
    "#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer).unwrap();
    let program = parser.parse_program().unwrap();

    match &program.body[0] {
        Statement::FunctionDeclaration(func) => match &func.body.body[0] {
            Statement::ReturnStatement(Some(Expression::Identifier(name))) => {
                assert_eq!(*name, "b");
            }
            other => panic!("unexpected return body: {other:?}"),
        },
        other => panic!("unexpected statement: {other:?}"),
    }
}
