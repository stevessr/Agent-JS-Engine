use std::fs;
use std::path::{Path, PathBuf};
use ai_agent::lexer::Lexer;
use ai_agent::parser::Parser;
use ai_agent::engine::Interpreter;

pub fn extract_test(code: &str) -> String {
    code.to_string()
}

pub fn run_test(path: &Path) -> (bool, bool) {
    let source = fs::read_to_string(path).unwrap();
    let code = extract_test(&source);
    let lexer = Lexer::new(&code);
    
    let parsed = match Parser::new(lexer) {
        Ok(mut parser) => {
            match parser.parse_program() {
                Ok(ast) => {
                    let mut interpreter = Interpreter::new();
                    let evaluated = interpreter.eval_program(&ast).is_ok();
                    (true, evaluated)
                }
                Err(_) => {
                    (false, false)
                }
            }
        },
        Err(_) => (false, false)
    };
    parsed
}

#[test]
fn test262_runner_main() {
    let test262_dir = Path::new("test262/test");
    if !test262_dir.exists() {
        println!("test262 directory not found!");
        return;
    }

    let mut total_attempts = 0;
    let mut parse_success = 0;
    let mut eval_success = 0;

    fn visit_dirs(dir: &Path, total: &mut usize, p_succ: &mut usize, e_succ: &mut usize) {
        if dir.is_dir() {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        visit_dirs(&path, total, p_succ, e_succ);
                    } else if path.extension().and_then(|s| s.to_str()) == Some("js") {
                        *total += 1;
                        let (parsed, evaluated) = run_test(&path);
                        if parsed { *p_succ += 1; }
                        if evaluated { *e_succ += 1; }
                    }
                }
            }
        }
    }

    visit_dirs(test262_dir, &mut total_attempts, &mut parse_success, &mut eval_success);

    println!("Total test attempted: {}", total_attempts);
    println!("Passed parsing: {}", parse_success);
    println!("Passed execution: {}", eval_success);
    println!("Parse Pass Rate: {:.2}%", parse_success as f64 / total_attempts as f64 * 100.0);
    println!("Eval Pass Rate: {:.2}%", eval_success as f64 / total_attempts as f64 * 100.0);
}
