use ai_agent::lexer::Lexer;

#[test]
fn single_test() {
    println!("Step 1");
    let source = "/ a";
    let mut lexer = Lexer::new(&source);
    println!("Step 2");
    let t = lexer.next_token();
    println!("Token: {:?}", t);
}
