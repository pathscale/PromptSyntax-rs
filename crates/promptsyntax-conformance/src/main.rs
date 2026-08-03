#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

use promptsyntax_conformance::{
    CheckReport, ValidationReport, check_requirements_json, check_schema_json,
    validate_instance_json,
};
use serde::Serialize;

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _program = args.next();
    let Some(command) = args.next() else {
        return usage();
    };
    match command.to_str() {
        Some("check-requirements") => {
            let Some(path) = args.next() else {
                return usage();
            };
            if args.next().is_some() {
                return usage();
            }
            let report = match fs::read(&path) {
                Ok(input) => check_requirements_json(&input),
                Err(error) => CheckReport::invalid_input("CORPUS_READ_FAILED", error.to_string()),
            };
            emit_report(&report, report.conformant)
        }
        Some("check-schema") => {
            let Some(path) = args.next() else {
                return usage();
            };
            if args.next().is_some() {
                return usage();
            }
            let report = match fs::read(&path) {
                Ok(input) => check_schema_json(&input),
                Err(error) => ValidationReport::invalid_input(
                    "schema",
                    "SCHEMA_READ_FAILED",
                    error.to_string(),
                ),
            };
            emit_report(&report, report.conformant)
        }
        Some("validate-instance") => {
            let Some(schema_path) = args.next() else {
                return usage();
            };
            let Some(instance_path) = args.next() else {
                return usage();
            };
            if args.next().is_some() {
                return usage();
            }
            let schema = match fs::read(&schema_path) {
                Ok(input) => input,
                Err(error) => {
                    let report = ValidationReport::invalid_input(
                        "instance",
                        "SCHEMA_READ_FAILED",
                        error.to_string(),
                    );
                    return emit_report(&report, false);
                }
            };
            let instance = match fs::read(&instance_path) {
                Ok(input) => input,
                Err(error) => {
                    let report = ValidationReport::invalid_input(
                        "instance",
                        "INSTANCE_READ_FAILED",
                        error.to_string(),
                    );
                    return emit_report(&report, false);
                }
            };
            let report = validate_instance_json(&schema, &instance);
            emit_report(&report, report.conformant)
        }
        _ => usage(),
    }
}

fn emit_report(report: &impl Serialize, conformant: bool) -> ExitCode {
    match serde_json::to_string_pretty(report) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("failed to serialize conformance report: {error}");
            return ExitCode::from(2);
        }
    }

    if conformant {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  ps-conformance check-requirements <requirements.json>\n  ps-conformance check-schema <schema.json>\n  ps-conformance validate-instance <schema.json> <instance.json>"
    );
    ExitCode::from(2)
}
