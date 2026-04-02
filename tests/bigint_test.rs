use ai_agent::lexer::{Lexer, Token};
use ai_agent::parser::{Parser, ast::*};

#[test]
fn lexer_parses_bigint_literals() {
    let mut lexer = Lexer::new("123n");
    assert_eq!(lexer.next_token().unwrap(), Token::BigInt("123"));

    let mut lexer = Lexer::new("0xFFn");
    assert_eq!(lexer.next_token().unwrap(), Token::BigInt("0xFF"));

    let mut lexer = Lexer::new("0o77n");
    assert_eq!(lexer.next_token().unwrap(), Token::BigInt("0o77"));

    let mut lexer = Lexer::new("0b1010n");
    assert_eq!(lexer.next_token().unwrap(), Token::BigInt("0b1010"));
}

#[test]
fn lexer_parses_numeric_separators() {
    let mut lexer = Lexer::new("1_000_000");
    assert_eq!(lexer.next_token().unwrap(), Token::Number(1000000.0));

    let mut lexer = Lexer::new("0xFF_FF");
    assert_eq!(lexer.next_token().unwrap(), Token::Number(65535.0));

    let mut lexer = Lexer::new("1_000n");
    assert_eq!(lexer.next_token().unwrap(), Token::BigInt("1_000"));
}

#[test]
fn parser_parses_bigint_literals() {
    let lexer = Lexer::new("123n");
    let mut parser = Parser::new(lexer).unwrap();
    let program = parser.parse_program().unwrap();

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::Literal(Literal::BigInt(n))) => {
            assert_eq!(*n, 123);
        }
        other => panic!("expected BigInt literal, got {:?}", other),
    }
}

#[test]
fn parser_parses_bigint_hex() {
    let lexer = Lexer::new("0xFFn");
    let mut parser = Parser::new(lexer).unwrap();
    let program = parser.parse_program().unwrap();

    match &program.body[0] {
        Statement::ExpressionStatement(Expression::Literal(Literal::BigInt(n))) => {
            assert_eq!(*n, 255);
        }
        other => panic!("expected BigInt literal, got {:?}", other),
    }
}

#[test]
fn parser_parses_numeric_separators() {
    let lexer = Lexer::new("const x = 1_000_000;");
    let mut parser = Parser::new(lexer).unwrap();
    let program = parser.parse_program().unwrap();

    match &program.body[0] {
        Statement::VariableDeclaration(decl) => match &decl.declarations[0].init {
            Some(Expression::Literal(Literal::Number(n))) => {
                assert_eq!(*n, 1000000.0);
            }
            other => panic!("expected number, got {:?}", other),
        },
        other => panic!("expected variable declaration, got {:?}", other),
    }
}
