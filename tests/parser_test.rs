use ai_agent::lexer::Lexer;
use ai_agent::parser::Parser;
use ai_agent::parser::ast::Statement;

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
