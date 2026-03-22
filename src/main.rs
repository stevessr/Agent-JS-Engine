use ai_agent::engine::{EvalOptions, JsEngine};
use std::env;
use std::fs;
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
    let mut inline_source = None;
    let mut file_path = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--eval" | "-e" => {
                let source = args
                    .next()
                    .ok_or_else(|| "missing argument for --eval".to_string())?;
                inline_source = Some(source);
            }
            "--strict" => {
                eval_options.strict = true;
            }
            "--test262" => {
                eval_options.bootstrap_test262 = true;
            }
            "--help" | "-h" => {
                print_usage();
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

    let source = match (inline_source, file_path) {
        (Some(source), None) => source,
        (None, Some(path)) => fs::read_to_string(&path)
            .map_err(|err| format!("failed to read {path}: {err}"))?,
        (Some(_), Some(_)) => {
            return Err("provide either a file path or --eval, not both".to_string());
        }
        (None, None) => {
            print_usage();
            return Ok(());
        }
    };

    let engine = JsEngine::new();
    let output = engine
        .eval_with_options(&source, &eval_options)
        .map_err(|err| err.to_string())?;

    for line in output.printed {
        println!("{line}");
    }

    if let Some(value) = output.value {
        println!("{value}");
    }

    Ok(())
}

fn print_usage() {
    println!("Usage: cargo run -- [--strict] [--test262] <file.js>");
    println!("       cargo run -- [--strict] [--test262] --eval \"1 + 2\"");
}
