use std::fs;
use std::path::PathBuf;
use ai_agent::lexer::Lexer;
use ai_agent::parser::Parser;
use ai_agent::engine::Interpreter;

fn main() {
    let mut total_attempts = 0;
    
    let mut stack = vec![PathBuf::from("test262/test")];
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    stack.push(entry.path());
                }
            }
        } else if path.extension().and_then(|s| s.to_str()) == Some("js") {
            total_attempts += 1;
            
            if total_attempts > 27000 {
                println!("Testing file: {:?}", path);
            }
            
            let code = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            
            let lexer = Lexer::new(&code);
            if let Ok(mut parser) = Parser::new(lexer) {
                if let Ok(ast) = parser.parse_program() {
                    let mut interpreter = Interpreter::new();
                    let _ = interpreter.eval_program(&ast);
                }
            }
        }
    }
}
