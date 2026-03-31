use ai_agent::lexer::Lexer;
use ai_agent::parser::{ast::*, Parser};

#[test]
fn parser_parses_async_generator_function() {
    let lexer = Lexer::new("async function* gen() { yield 1; }");
    let mut parser = Parser::new(lexer).unwrap();
    let program = parser.parse_program().unwrap();
    
    match &program.body[0] {
        Statement::FunctionDeclaration(func) => {
            assert!(func.is_async);
            assert!(func.is_generator);
            assert_eq!(func.id, Some("gen"));
        }
        other => panic!("expected async generator function, got {:?}", other),
    }
}

#[test]
fn parser_parses_async_generator_expression() {
    let lexer = Lexer::new("const f = async function* () { yield 1; };");
    let mut parser = Parser::new(lexer).unwrap();
    let program = parser.parse_program().unwrap();
    
    match &program.body[0] {
        Statement::VariableDeclaration(decl) => {
            match &decl.declarations[0].init {
                Some(Expression::FunctionExpression(func)) => {
                    assert!(func.is_async);
                    assert!(func.is_generator);
                }
                other => panic!("expected async generator expression, got {:?}", other),
            }
        }
        other => panic!("expected variable declaration, got {:?}", other),
    }
}

#[test]
fn parser_parses_async_generator_method() {
    let lexer = Lexer::new("class C { async *gen() { yield 1; } }");
    let mut parser = Parser::new(lexer).unwrap();
    let program = parser.parse_program().unwrap();
    
    match &program.body[0] {
        Statement::ClassDeclaration(class) => {
            match &class.body[0] {
                ClassElement::Method { value, .. } => {
                    assert!(value.is_async);
                    assert!(value.is_generator);
                }
                other => panic!("expected async generator method, got {:?}", other),
            }
        }
        other => panic!("expected class declaration, got {:?}", other),
    }
}
