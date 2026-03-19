import re

with open("tests/test262_runner.rs", "r") as f:
    text = f.read()

# We want to replace the error printing to aggregate it.
text = text.replace('println!("Parse fail: {:?}", e);', 'parse_errors.entry(format!("{:?}", e)).and_modify(|e| *e += 1).or_insert(1);')
text = text.replace('println!("Eval fail: {:?}", e);', 'eval_errors.entry(format!("{:?}", e)).and_modify(|e| *e += 1).or_insert(1);')

text = text.replace('let mut parse_success = 0;', 'let mut parse_success = 0;\n    let mut parse_errors: std::collections::HashMap<String, usize> = std::collections::HashMap::new();\n    let mut eval_errors: std::collections::HashMap<String, usize> = std::collections::HashMap::new();')

tail = """    println!("Parse Pass Rate: {:.2}%", parse_success as f64 / total_attempts as f64 * 100.0);
    println!("Eval Pass Rate: {:.2}%", eval_success as f64 / total_attempts as f64 * 100.0);

    let mut parse_err_vec: Vec<_> = parse_errors.into_iter().collect();
    parse_err_vec.sort_by(|a, b| b.1.cmp(&a.1));
    println!("Top 10 Parse Errors:");
    for (err, count) in parse_err_vec.iter().take(10) {
        println!("{}: {}", count, err);
    }
    
    let mut eval_err_vec: Vec<_> = eval_errors.into_iter().collect();
    eval_err_vec.sort_by(|a, b| b.1.cmp(&a.1));
    println!("Top 10 Eval Errors:");
    for (err, count) in eval_err_vec.iter().take(10) {
        println!("{}: {}", count, err);
    }"""

text = re.sub(r'println!\("Parse Pass Rate:(.|\n)*$', tail + "\n}", text)

with open("tests/test262_runner.rs", "w") as f:
    f.write(text)
