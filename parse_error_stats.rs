use ai_agent::lexer::Lexer;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn main() {
    let mut total = 0;
    let mut passed = 0;
    let mut general_failures = HashMap::new();

    let mut stack = vec![PathBuf::from("test262/test")];
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    stack.push(entry.path());
                }
            }
        } else if path.extension().and_then(|s| s.to_str()) == Some("js") {
            total += 1;
            let code = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let lexer = Lexer::new(&code);
            match ai_agent::parser::Parser::new(lexer) {
                Ok(mut parser) => {
                    match parser.parse_program() {
                        Ok(_) => passed += 1,
                        Err(e) => {
                            let mut err_str = e.to_string();
                            // Simplify error string to group them better
                            if err_str.starts_with("Unexpected token: expected ") {
                                if let Some(pos) = err_str.find(", found None") {
                                    err_str = err_str[..pos].to_string() + ", found None";
                                } else if let Some(pos) = err_str.find(", found Some(") {
                                    let end = err_str.len() - 2;
                                    let token_str = &err_str[pos + 13..end];
                                    let basic_type = token_str
                                        .split('(')
                                        .next()
                                        .unwrap_or(token_str)
                                        .to_string();
                                    err_str = err_str[..pos].to_string()
                                        + &format!(", found Some({})", basic_type);
                                }
                            }
                            *general_failures.entry(err_str).or_insert(0) += 1;
                        }
                    }
                }
                Err(e) => {
                    *general_failures
                        .entry(format!("Lexer error: {:?}", e))
                        .or_insert(0) += 1;
                }
            }
        }
    }

    println!("Total scripts parsed: {}", total);
    println!(
        "Parse Pass Rate: {:.2}% ({}/{})",
        passed as f64 / total as f64 * 100.0,
        passed,
        total
    );

    let mut err_vec: Vec<_> = general_failures.into_iter().collect();
    err_vec.sort_by(|a, b| b.1.cmp(&a.1));

    println!("\nTop 20 Parse Errors Overall:");
    for (err, counts) in err_vec.iter().take(20) {
        println!("{:>6} | {}", counts, err);
    }
}
