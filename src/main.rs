use ai_agent::engine::{EvalOptions, JsEngine, ReplSession};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::env;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mut eval_options = EvalOptions::default();
    let mut is_module = false;
    let mut inline_source = None;
    let mut file_path = None;
    let mut interactive = false;
    let mut print_result = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--eval" | "-e" => {
                let source = args
                    .next()
                    .ok_or_else(|| "missing argument for --eval".to_string())?;
                inline_source = Some(source);
            }
            "--print" | "-p" => {
                print_result = true;
                let source = args
                    .next()
                    .ok_or_else(|| "missing argument for --print".to_string())?;
                inline_source = Some(source);
            }
            "--strict" => {
                eval_options.strict = true;
            }
            "--module" | "-m" => {
                is_module = true;
            }
            "--test262" => {
                eval_options.bootstrap_test262 = true;
            }
            "-i" | "--interactive" => {
                interactive = true;
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            "--version" | "-v" => {
                println!("ai-agent v0.1.0 (boa_engine 0.21.0)");
                return Ok(());
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown flag: {arg}"));
            }
            _ => {
                file_path = Some(arg);
            }
        }
    }

    match (inline_source, file_path, interactive) {
        // --eval or -p with source
        (Some(source), None, false) => {
            run_inline(&source, is_module, &eval_options, print_result)
        }
        // File execution
        (None, Some(path), false) => run_file(&path, is_module, &eval_options),
        // --eval + -i: run eval then enter REPL
        (Some(source), None, true) => {
            run_inline(&source, is_module, &eval_options, print_result)?;
            run_repl(&eval_options)
        }
        // File + -i: run file then enter REPL
        (None, Some(path), true) => {
            run_file(&path, is_module, &eval_options)?;
            run_repl(&eval_options)
        }
        (Some(_), Some(_), _) => {
            Err("provide either a file path or --eval, not both".to_string())
        }
        // No file, no eval: REPL mode or stdin
        (None, None, _) => {
            if io::stdin().is_terminal() {
                run_repl(&eval_options)
            } else {
                run_stdin(is_module, &eval_options)
            }
        }
    }
}

fn run_inline(
    source: &str,
    is_module: bool,
    options: &EvalOptions,
    print_result: bool,
) -> Result<(), String> {
    let inline_path = env::current_dir()
        .map_err(|err| format!("failed to resolve current directory: {err}"))?
        .join("__inline__.mjs");
    let module_root = inline_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    let engine = JsEngine::new();
    let output = if is_module {
        engine
            .eval_module_with_options(source, &inline_path, module_root, options)
            .map_err(|err| err.to_string())?
    } else {
        engine
            .eval_script_with_options(source, &inline_path, module_root, options)
            .map_err(|err| err.to_string())?
    };

    for line in output.printed {
        println!("{line}");
    }

    if print_result {
        if let Some(value) = output.value {
            println!("{value}");
        }
    }

    Ok(())
}

fn run_file(path: &str, is_module: bool, options: &EvalOptions) -> Result<(), String> {
    let source =
        fs::read_to_string(path).map_err(|err| format!("failed to read {path}: {err}"))?;
    let source_path = PathBuf::from(path);
    let module_root = source_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    let engine = JsEngine::new();
    let output = if is_module {
        engine
            .eval_module_with_options(&source, &source_path, module_root, options)
            .map_err(|err| err.to_string())?
    } else {
        engine
            .eval_script_with_options(&source, &source_path, module_root, options)
            .map_err(|err| err.to_string())?
    };

    for line in output.printed {
        println!("{line}");
    }

    Ok(())
}

fn run_stdin(is_module: bool, options: &EvalOptions) -> Result<(), String> {
    let source = io::read_to_string(io::stdin())
        .map_err(|err| format!("failed to read stdin: {err}"))?;
    run_inline(&source, is_module, options, false)
}

fn run_repl(options: &EvalOptions) -> Result<(), String> {
    println!("Welcome to ai-agent REPL (boa_engine 0.21.0)");
    println!("Type .help for available commands, .exit or Ctrl+D to exit\n");

    let mut rl = DefaultEditor::new().map_err(|e| e.to_string())?;
    let history_path = dirs_history_path();
    if let Some(ref path) = history_path {
        let _ = rl.load_history(path);
    }

    let cwd = env::current_dir()
        .map_err(|err| format!("failed to resolve current directory: {err}"))?;
    let mut session = ReplSession::new(&cwd, options).map_err(|e| e.to_string())?;

    let mut multiline_buffer = String::new();
    let mut in_multiline = false;

    loop {
        let prompt = if in_multiline { "... " } else { "> " };
        match rl.readline(prompt) {
            Ok(line) => {
                // Handle dot commands
                if !in_multiline && line.starts_with('.') {
                    match handle_dot_command(&line, &mut session, options) {
                        DotCommandResult::Continue => continue,
                        DotCommandResult::Exit => break,
                        DotCommandResult::Unknown(cmd) => {
                            eprintln!("Unknown command: {cmd}. Type .help for available commands.");
                            continue;
                        }
                    }
                }

                // Accumulate input
                if in_multiline {
                    multiline_buffer.push('\n');
                    multiline_buffer.push_str(&line);
                } else {
                    multiline_buffer = line.clone();
                }

                // Check if input is complete
                if is_incomplete_input(&multiline_buffer) {
                    in_multiline = true;
                    continue;
                }

                in_multiline = false;
                let input = std::mem::take(&mut multiline_buffer);

                if input.trim().is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(&input);

                match session.eval(&input) {
                    Ok(output) => {
                        for line in output.printed {
                            println!("{line}");
                        }
                        if let Some(value) = output.value {
                            println!("\x1b[90m{value}\x1b[0m");
                        }
                    }
                    Err(err) => {
                        eprintln!("\x1b[31m{err}\x1b[0m");
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                if in_multiline {
                    println!("^C");
                    multiline_buffer.clear();
                    in_multiline = false;
                } else {
                    println!("(To exit, press Ctrl+D or type .exit)");
                }
            }
            Err(ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(err) => {
                eprintln!("Error: {err}");
                break;
            }
        }
    }

    if let Some(ref path) = history_path {
        let _ = rl.save_history(path);
    }

    Ok(())
}

enum DotCommandResult {
    Continue,
    Exit,
    Unknown(String),
}

fn handle_dot_command(
    line: &str,
    session: &mut ReplSession,
    _options: &EvalOptions,
) -> DotCommandResult {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let cmd = parts.first().map(|s| *s).unwrap_or("");

    match cmd {
        ".help" => {
            println!("REPL Commands:");
            println!("  .help      Show this help message");
            println!("  .exit      Exit the REPL");
            println!("  .clear     Clear the REPL context");
            println!("  .load <f>  Load and execute a JavaScript file");
            println!();
            println!("Keyboard:");
            println!("  Ctrl+C     Cancel current input");
            println!("  Ctrl+D     Exit the REPL");
            DotCommandResult::Continue
        }
        ".exit" | ".quit" => DotCommandResult::Exit,
        ".clear" => {
            let cwd = env::current_dir().unwrap_or_default();
            match ReplSession::new(&cwd, _options) {
                Ok(new_session) => {
                    *session = new_session;
                    println!("REPL context cleared.");
                }
                Err(e) => eprintln!("Failed to clear context: {e}"),
            }
            DotCommandResult::Continue
        }
        ".load" => {
            if parts.len() < 2 {
                eprintln!("Usage: .load <filename>");
            } else {
                let path = parts[1];
                match fs::read_to_string(path) {
                    Ok(source) => match session.eval(&source) {
                        Ok(output) => {
                            for line in output.printed {
                                println!("{line}");
                            }
                            if let Some(value) = output.value {
                                println!("\x1b[90m{value}\x1b[0m");
                            }
                        }
                        Err(e) => eprintln!("\x1b[31m{e}\x1b[0m"),
                    },
                    Err(e) => eprintln!("Failed to load file: {e}"),
                }
            }
            DotCommandResult::Continue
        }
        _ => DotCommandResult::Unknown(cmd.to_string()),
    }
}

fn is_incomplete_input(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Count brackets
    let mut brace_count = 0i32;
    let mut bracket_count = 0i32;
    let mut paren_count = 0i32;
    let mut in_string = false;
    let mut string_char = ' ';
    let mut escape_next = false;
    let mut in_template = false;
    let mut template_depth = 0i32;

    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        if escape_next {
            escape_next = false;
            i += 1;
            continue;
        }

        if ch == '\\' && (in_string || in_template) {
            escape_next = true;
            i += 1;
            continue;
        }

        if in_string {
            if ch == string_char {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if in_template {
            if ch == '`' && template_depth == 0 {
                in_template = false;
            } else if ch == '$' && i + 1 < chars.len() && chars[i + 1] == '{' {
                template_depth += 1;
                i += 1;
            } else if ch == '}' && template_depth > 0 {
                template_depth -= 1;
            }
            i += 1;
            continue;
        }

        match ch {
            '"' | '\'' => {
                in_string = true;
                string_char = ch;
            }
            '`' => {
                in_template = true;
            }
            '{' => brace_count += 1,
            '}' => brace_count -= 1,
            '[' => bracket_count += 1,
            ']' => bracket_count -= 1,
            '(' => paren_count += 1,
            ')' => paren_count -= 1,
            '/' if i + 1 < chars.len() => {
                // Skip comments
                if chars[i + 1] == '/' {
                    // Single line comment - find end
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    continue;
                } else if chars[i + 1] == '*' {
                    // Multi-line comment
                    i += 2;
                    while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                        i += 1;
                    }
                    i += 2;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }

    // Incomplete if unclosed brackets/strings
    in_string || in_template || brace_count > 0 || bracket_count > 0 || paren_count > 0
}

fn dirs_history_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("ai-agent").join("repl_history"))
}

fn print_usage() {
    println!("ai-agent - JavaScript engine powered by boa_engine");
    println!();
    println!("Usage: ai-agent [options] [script.js] [arguments]");
    println!("       ai-agent [options] --eval \"code\"");
    println!("       ai-agent                              # Start REPL");
    println!();
    println!("Options:");
    println!("  -e, --eval <code>    Evaluate JavaScript code");
    println!("  -p, --print <code>   Evaluate and print result");
    println!("  -m, --module         Treat input as ES module");
    println!("  -i, --interactive    Enter REPL after running script");
    println!("  --strict             Enable strict mode");
    println!("  --test262            Enable test262 harness globals");
    println!("  -v, --version        Print version information");
    println!("  -h, --help           Show this help message");
}
