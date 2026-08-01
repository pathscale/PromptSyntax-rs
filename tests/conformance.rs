use promptsyntax::{DiagnosticCode, Directive, Parser};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    source: String,
    #[serde(default)]
    options: Options,
    data_plane: String,
    directives: Vec<String>,
    diagnostics: Vec<String>,
}

#[derive(Default, Deserialize)]
struct Options {
    #[serde(default)]
    entities: Vec<String>,
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    authoring_namespaces: Vec<String>,
}

#[test]
fn shared_conformance_corpus() {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("conformance.json")).expect("valid corpus");
    for case in cases {
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
        assert_eq!(parsed.round_trip(), case.source, "{} round trip", case.name);
        assert_eq!(parsed.data_plane(), case.data_plane, "{} data", case.name);
        assert_eq!(
            parsed
                .directives()
                .map(|segment| directive_kind(&segment.directive))
                .collect::<Vec<_>>(),
            case.directives,
            "{} directives",
            case.name
        );
        assert_eq!(
            parsed
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic_code(diagnostic.code))
                .collect::<Vec<_>>(),
            case.diagnostics,
            "{} diagnostics",
            case.name
        );
    }
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
