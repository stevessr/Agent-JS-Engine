use ai_agent::lexer::Lexer;
use ai_agent::parser::Parser;

fn main() {
    let mut lexer = Lexer::new("++x");
    while let Ok(t) = lexer.next_token() {
        if t == ai_agent::lexer::Token::Eof { break; }
        println!("{:?}", t);
    }
}
