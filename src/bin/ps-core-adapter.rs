#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

use promptsyntax::{
    Action, Argument, DiagnosticCode, Directive, Parser, Reference, Scalar, Segment, SourceSpan,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    #[serde(rename = "name")]
    _name: String,
    source: String,
    #[serde(default)]
    options: Options,
    #[serde(default)]
    data_plane: Option<String>,
    #[serde(default)]
    directives: Option<Vec<String>>,
    #[serde(default)]
    diagnostics: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    #[serde(default)]
    entities: Vec<String>,
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    authoring_namespaces: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AdapterResult {
    format_version: &'static str,
    target: &'static str,
    implementation: Implementation,
    results: Vec<CaseResult>,
}

#[derive(Debug, Serialize)]
struct AdapterHeader {
    format_version: &'static str,
    target: &'static str,
    implementation: Implementation,
}

#[derive(Clone, Debug, Serialize)]
struct Implementation {
    id: &'static str,
    version: &'static str,
    commit: String,
}

#[derive(Debug, Serialize)]
struct CaseResult {
    case_id: String,
    conformant: bool,
    diagnostics: Vec<&'static str>,
    output: CoreOutput,
}

#[derive(Debug, Serialize)]
struct CoreOutput {
    round_trip: String,
    data_plane: String,
    segments: Vec<Value>,
    directives: Vec<Value>,
    parser_diagnostics: Vec<Value>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(&error),
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args_os();
    let _program = args.next();
    let cases_path = args
        .next()
        .ok_or_else(|| "usage: ps-core-adapter <core-cases.json> <commit>".to_owned())?;
    let commit = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| "usage: ps-core-adapter <core-cases.json> <commit>".to_owned())?;
    let json_lines = match args.next() {
        None => false,
        Some(flag) if flag == "--jsonl" => true,
        Some(_) => {
            return Err("usage: ps-core-adapter <core-cases.json> <commit> [--jsonl]".to_owned());
        }
    };
    if args.next().is_some() {
        return Err("usage: ps-core-adapter <core-cases.json> <commit>".to_owned());
    }
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("commit must be a 40-character hexadecimal Git object id".to_owned());
    }

    let input = fs::read(&cases_path)
        .map_err(|error| format!("failed to read {}: {error}", cases_path.to_string_lossy()))?;
    let cases: Vec<Case> = serde_json::from_slice(&input)
        .map_err(|error| format!("failed to parse canonical cases: {error}"))?;
    let implementation = Implementation {
        id: "promptsyntax-rs",
        version: env!("CARGO_PKG_VERSION"),
        commit,
    };
    if json_lines {
        let header = AdapterHeader {
            format_version: "0.1-draft",
            target: "core-parser",
            implementation,
        };
        println!(
            "{}",
            serde_json::to_string(&header)
                .map_err(|error| format!("failed to serialize adapter header: {error}"))?
        );
        for case in cases {
            println!(
                "{}",
                serde_json::to_string(&run_case(case))
                    .map_err(|error| format!("failed to serialize adapter case: {error}"))?
            );
        }
    } else {
        let result = AdapterResult {
            format_version: "0.1-draft",
            target: "core-parser",
            implementation,
            results: cases.into_iter().map(run_case).collect(),
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&result)
                .map_err(|error| format!("failed to serialize adapter output: {error}"))?
        );
    }
    Ok(())
}

fn run_case(case: Case) -> CaseResult {
    let mut parser = Parser::new();
    for entity in case.options.entities {
        parser = parser.entity(entity);
    }
    for action in case.options.actions {
        parser = parser.action(action);
    }
    for namespace in case.options.authoring_namespaces {
        parser = parser.authoring_namespace(namespace);
    }

    let parsed = parser.parse(&case.source);
    let round_trip = parsed.round_trip();
    let data_plane = parsed.data_plane();
    let directive_kinds = parsed
        .directives()
        .map(|segment| directive_kind(&segment.directive))
        .collect::<Vec<_>>();
    let diagnostic_codes = parsed
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic_code(diagnostic.code))
        .collect::<Vec<_>>();

    let mut adapter_diagnostics = Vec::new();
    if round_trip != case.source {
        adapter_diagnostics.push("CORE_ROUND_TRIP_MISMATCH");
    }
    match (&case.data_plane, &case.directives, &case.diagnostics) {
        (Some(expected_data), Some(expected_directives), Some(expected_diagnostics)) => {
            if &data_plane != expected_data {
                adapter_diagnostics.push("CORE_DATA_PLANE_MISMATCH");
            }
            if &directive_kinds != expected_directives {
                adapter_diagnostics.push("CORE_DIRECTIVE_SEQUENCE_MISMATCH");
            }
            if &diagnostic_codes != expected_diagnostics {
                adapter_diagnostics.push("CORE_DIAGNOSTIC_SEQUENCE_MISMATCH");
            }
        }
        (None, None, None) => {}
        _ => adapter_diagnostics.push("CORE_CASE_EXPECTATION_INCOMPLETE"),
    }

    let output = CoreOutput {
        round_trip,
        data_plane,
        segments: parsed
            .segments
            .iter()
            .map(|segment| segment_json(segment, &case.source))
            .collect(),
        directives: parsed
            .directives()
            .map(|segment| {
                json!({
                    "span": span_json(segment.span, &case.source),
                    "source": segment.source,
                    "directive": directive_json(&segment.directive, &case.source),
                })
            })
            .collect(),
        parser_diagnostics: parsed
            .diagnostics
            .iter()
            .map(|diagnostic| {
                json!({
                    "code": diagnostic_code(diagnostic.code),
                    "span": span_json(diagnostic.span, &case.source),
                })
            })
            .collect(),
    };

    CaseResult {
        case_id: case.id,
        conformant: adapter_diagnostics.is_empty(),
        diagnostics: adapter_diagnostics,
        output,
    }
}

fn segment_json(segment: &Segment, source: &str) -> Value {
    match segment {
        Segment::Text(text) => json!({
            "type": "text",
            "span": span_json(text.span, source),
            "source": text.text,
        }),
        Segment::Directive(directive) => json!({
            "type": "directive",
            "span": span_json(directive.span, source),
            "source": directive.source,
            "directive": directive_json(&directive.directive, source),
        }),
    }
}

fn directive_json(directive: &Directive, source: &str) -> Value {
    match directive {
        Directive::Reference(reference) => json!({
            "kind": "reference",
            "reference": reference_json(reference),
        }),
        Directive::Action(action) => json!({
            "kind": "action",
            "action": action_json(action),
        }),
        Directive::Route(route) => json!({
            "kind": "route",
            "route": {
                "steps": route.steps.iter().map(|step| json!({
                    "reference": reference_json(&step.reference),
                    "limits": step.limits.iter().map(argument_json).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
                "terminal": route.terminal.map(|terminal| match terminal {
                    promptsyntax::RouteTerminal::Ask => "ask",
                    promptsyntax::RouteTerminal::Fail => "fail",
                }),
            },
        }),
        Directive::Span { header, segments } => json!({
            "kind": "span",
            "header": {
                "raw": header.raw,
                "references": header.references.iter().map(reference_json).collect::<Vec<_>>(),
                "attributes": header.attributes.iter().map(|attribute| json!({
                    "key": attribute.key,
                    "value": scalar_json(&attribute.value),
                })).collect::<Vec<_>>(),
            },
            "segments": segments.iter().map(|segment| segment_json(segment, source)).collect::<Vec<_>>(),
        }),
        Directive::AuthoringSegment { reference } => json!({
            "kind": "authoring_segment",
            "reference": reference_json(reference),
        }),
        Directive::InvalidAuthoringSegment { header } => json!({
            "kind": "invalid_authoring_segment",
            "header": header,
        }),
        Directive::Frontmatter { body } => json!({
            "kind": "frontmatter",
            "body": body,
        }),
    }
}

fn reference_json(reference: &Reference) -> Value {
    json!({
        "namespace": reference.namespace,
        "name": reference.name,
        "version": reference.version,
        "strict": reference.strict,
        "arguments": reference.arguments.iter().map(argument_json).collect::<Vec<_>>(),
    })
}

fn action_json(action: &Action) -> Value {
    json!({
        "name": action.name,
        "arguments": action.arguments.iter().map(argument_json).collect::<Vec<_>>(),
    })
}

fn argument_json(argument: &Argument) -> Value {
    json!({
        "key": argument.key,
        "value": scalar_json(&argument.value),
    })
}

fn scalar_json(scalar: &Scalar) -> Value {
    match scalar {
        Scalar::String(value) => json!({ "kind": "string", "value": value }),
        Scalar::Number(value) => json!({ "kind": "number", "value": value }),
        Scalar::Boolean(value) => json!({ "kind": "boolean", "value": value }),
        Scalar::Null => json!({ "kind": "null" }),
        Scalar::Bare(value) => json!({ "kind": "bare", "value": value }),
    }
}

fn span_json(span: SourceSpan, source: &str) -> Value {
    json!({
        "start": span.start,
        "end": span.end,
        "source": source
            .get(span.start..span.end)
            .expect("parser emitted an invalid UTF-8 byte span"),
    })
}

fn directive_kind(directive: &Directive) -> &'static str {
    match directive {
        Directive::Reference(_) => "reference",
        Directive::Action(_) => "action",
        Directive::Route(_) => "route",
        Directive::Span { .. } => "span",
        Directive::AuthoringSegment { .. } => "authoring_segment",
        Directive::InvalidAuthoringSegment { .. } => "invalid_authoring_segment",
        Directive::Frontmatter { .. } => "frontmatter",
    }
}

const fn diagnostic_code(code: DiagnosticCode) -> &'static str {
    match code {
        DiagnosticCode::SyntaxInvalid => "SYNTAX_INVALID",
        DiagnosticCode::BidiControl => "BIDI_CONTROL",
        DiagnosticCode::UnclosedSpan => "UNCLOSED_SPAN",
        DiagnosticCode::UnclosedFrontmatter => "UNCLOSED_FRONTMATTER",
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::from(2)
}
