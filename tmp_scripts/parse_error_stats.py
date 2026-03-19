import re

with open("parse_error_stats.rs", "r") as f:
    content = f.read()

replacement = """
    // Print pass rate
    println!("Parse Pass Rate: {:.2}% ({}/{})", success_count as f64 / total_files as f64 * 100.0, success_count, total_files);

    // Collect and sort all errors by count
    let mut error_counts_vec: Vec<_> = error_counts.into_iter().collect();
    error_counts_vec.sort_by(|a, b| b.1.cmp(&a.1));

    println!("\\nTop 20 Parse Errors Overall:");
    for (err_msg, count) in error_counts_vec.into_iter().take(20) {
        println!("{:>6} | {}", count, err_msg);
    }
"""

content = re.sub(
    r"    // Collect and sort.+?(?=\n})",
    replacement,
    content,
    flags=re.DOTALL
)

with open("parse_error_stats.rs", "w") as f:
    f.write(content)

