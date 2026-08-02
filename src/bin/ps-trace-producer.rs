#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

use promptsyntax::trace::produce_trace_json;
use serde_json::json;

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _program = args.next();
    let Some(path) = args.next() else {
        eprintln!("usage: ps-trace-producer <producer-input.json>");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("usage: ps-trace-producer <producer-input.json>");
        return ExitCode::from(2);
    }
    let input = match fs::read(&path) {
        Ok(input) => input,
        Err(error) => {
            eprintln!("failed to read {}: {error}", path.to_string_lossy());
            return ExitCode::from(2);
        }
    };
    match produce_trace_json(&input) {
        Ok(trace) => emit(&trace, ExitCode::SUCCESS),
        Err(error) => emit(
            &json!({ "format_version": "0.1-draft", "error": error }),
            ExitCode::from(1),
        ),
    }
}

fn emit(value: &serde_json::Value, exit: ExitCode) -> ExitCode {
    match serde_json::to_string_pretty(value) {
        Ok(output) => {
            println!("{output}");
            exit
        }
        Err(error) => {
            eprintln!("failed to serialize producer output: {error}");
            ExitCode::from(2)
        }
    }
}
