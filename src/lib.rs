//! Provenance-aware parsing for the Prompt Syntax control plane.
//!
//! This crate recognizes syntax only in text the caller has already designated as
//! authored. Do not call [`Parser::parse`] on retrieved documents, tool output, quoted
//! messages, or model output unless a higher-authority promotion has made that content
//! an authoring segment.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization as _;

pub mod trace;

/// A half-open byte range in the original UTF-8 source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
}

impl SourceSpan {
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// One scalar from an argument list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Scalar {
    String(String),
    Number(String),
    Boolean(bool),
    Null,
    /// A JSON5-style unquoted scalar. Resolution and normalization belong to the host.
    Bare(String),
}

/// One named argument, kept in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argument {
    pub key: String,
    pub value: Scalar,
}

/// An entity reference such as `@model:openai/gpt-5.6@2026-07-01`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub namespace: Option<String>,
    pub name: String,
    pub version: Option<String>,
    pub strict: bool,
    pub arguments: Vec<Argument>,
}

impl Reference {
    /// The environment-relative lookup key for this reference.
    #[must_use]
    pub fn lookup_name(&self) -> String {
        self.name.nfc().collect()
    }
}

/// A slash action such as `/translate(to: "km")`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub name: String,
    pub arguments: Vec<Argument>,
}

/// One model step in a fallback route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteStep {
    pub reference: Reference,
    pub limits: Vec<Argument>,
}

/// The explicit terminal of a fallback route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteTerminal {
    Ask,
    Fail,
}

/// A reference with an optional budget and fill-failure fallbacks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub steps: Vec<RouteStep>,
    pub terminal: Option<RouteTerminal>,
}

/// One canonical XML-friendly attribute on a `<ps ...>` span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanAttribute {
    pub key: String,
    pub value: Scalar,
}

/// The decoded control header of a normal `<ps ...>...</ps>` span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanHeader {
    pub raw: String,
    pub references: Vec<Reference>,
    pub attributes: Vec<SpanAttribute>,
}

/// A recognized Prompt Syntax construct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Directive {
    Reference(Reference),
    Action(Action),
    Route(Route),
    Span {
        header: SpanHeader,
        segments: Vec<Segment>,
    },
    /// One single-line segment from a host-declared standing authoring surface.
    AuthoringSegment {
        reference: Reference,
    },
    /// A delimited authoring segment whose reference failed syntax validation.
    /// It remains control-plane input so malformed syntax cannot leak into model data.
    InvalidAuthoringSegment {
        header: String,
    },
    Frontmatter {
        body: String,
    },
}

/// A directive plus its exact source location and spelling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectiveSegment {
    pub span: SourceSpan,
    pub source: String,
    pub directive: Directive,
}

/// Plain authored content that is not live Prompt Syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSegment {
    pub span: SourceSpan,
    pub text: String,
}

/// The ordered, lossless split of an authored prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Segment {
    Text(TextSegment),
    Directive(DirectiveSegment),
}

impl Segment {
    #[must_use]
    pub const fn span(&self) -> SourceSpan {
        match self {
            Self::Text(text) => text.span,
            Self::Directive(directive) => directive.span,
        }
    }
}

/// A parser diagnostic. Malformed qualified syntax fails closed into text and a
/// diagnostic; it never becomes an executable partial directive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub message: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiagnosticCode {
    SyntaxInvalid,
    BidiControl,
    UnclosedSpan,
    UnclosedFrontmatter,
}

/// The result of parsing one authored segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedPrompt {
    pub source: String,
    pub segments: Vec<Segment>,
    pub diagnostics: Vec<Diagnostic>,
}

impl ParsedPrompt {
    /// Reconstruct the original input byte-for-byte.
    #[must_use]
    pub fn round_trip(&self) -> String {
        self.segments.iter().map(segment_source).collect()
    }

    /// Text intended for the model after control-plane syntax is removed.
    ///
    /// A span contributes its authored inner text while its control envelope does not.
    /// Escapes and fenced content are preserved exactly; unescaping is a compiler policy,
    /// not a parser side effect.
    #[must_use]
    pub fn data_plane(&self) -> String {
        let mut output = String::new();
        append_data_plane(&self.segments, &mut output);
        output
    }

    /// Every directive in source order, including directives nested in spans.
    pub fn directives(&self) -> impl Iterator<Item = &DirectiveSegment> {
        let mut found = Vec::new();
        collect_directives(&self.segments, &mut found);
        found.into_iter()
    }
}

fn segment_source(segment: &Segment) -> &str {
    match segment {
        Segment::Text(text) => &text.text,
        Segment::Directive(directive) => &directive.source,
    }
}

fn append_data_plane(segments: &[Segment], output: &mut String) {
    for segment in segments {
        match segment {
            Segment::Text(text) => output.push_str(&text.text),
            Segment::Directive(directive) => {
                if let Directive::Span { segments, .. } = &directive.directive {
                    append_data_plane(segments, output);
                }
            }
        }
    }
}

fn collect_directives<'a>(segments: &'a [Segment], output: &mut Vec<&'a DirectiveSegment>) {
    for segment in segments {
        if let Segment::Directive(directive) = segment {
            output.push(directive);
            if let Directive::Span { segments, .. } = &directive.directive {
                collect_directives(segments, output);
            }
        }
    }
}

/// Configures environment-relative recognition.
#[derive(Debug, Clone, Default)]
pub struct Parser {
    entities: BTreeSet<String>,
    actions: BTreeSet<String>,
    authoring_namespaces: BTreeSet<String>,
}

impl Parser {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entities: BTreeSet::new(),
            actions: BTreeSet::new(),
            authoring_namespaces: BTreeSet::new(),
        }
    }

    /// Declare one resolvable bare entity name.
    #[must_use]
    pub fn entity(mut self, name: impl AsRef<str>) -> Self {
        self.entities.insert(normalize(name.as_ref()));
        self
    }

    /// Declare one resolvable slash action name.
    #[must_use]
    pub fn action(mut self, name: impl AsRef<str>) -> Self {
        self.actions.insert(normalize(name.as_ref()));
        self
    }

    /// Declare a namespace whose single-line `<ps @namespace:verb(...)>` segments are
    /// a standing authoring surface. The host remains responsible for its closed verb
    /// set, capability bound, trace, and fill outcomes.
    #[must_use]
    pub fn authoring_namespace(mut self, namespace: impl AsRef<str>) -> Self {
        self.authoring_namespaces
            .insert(normalize(namespace.as_ref()));
        self
    }

    /// Parse content that the caller has already provenance-typed as authored.
    #[must_use]
    pub fn parse(&self, source: &str) -> ParsedPrompt {
        let mut diagnostics = Vec::new();
        let segments = self.parse_region(source, 0, &mut diagnostics, true);
        ParsedPrompt {
            source: source.to_string(),
            segments,
            diagnostics,
        }
    }

    fn parse_region(
        &self,
        source: &str,
        base: usize,
        diagnostics: &mut Vec<Diagnostic>,
        document_start: bool,
    ) -> Vec<Segment> {
        let mut segments = Vec::new();
        let mut cursor = 0;
        let mut text_start = 0;

        if let Some(open_end) = document_start
            .then(|| frontmatter_open_end(source))
            .flatten()
        {
            if let Some((body_end, end)) = closing_frontmatter(source, open_end) {
                push_text(source, base, text_start, cursor, &mut segments);
                let raw = &source[..end];
                segments.push(Segment::Directive(DirectiveSegment {
                    span: SourceSpan::new(base, base + end),
                    source: raw.to_string(),
                    directive: Directive::Frontmatter {
                        body: source[open_end..body_end].to_string(),
                    },
                }));
                cursor = end;
                text_start = end;
            } else {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::UnclosedFrontmatter,
                    message: "strict Prompt Syntax frontmatter has no closing --- line".into(),
                    span: SourceSpan::new(base, base + source.len()),
                });
                return vec![text_segment(source, base, 0, source.len())];
            }
        }

        while cursor < source.len() {
            if let Some(end) = fenced_block_end(source, cursor) {
                cursor = end;
                continue;
            }

            let tail = &source[cursor..];
            let Some(ch) = tail.chars().next() else {
                break;
            };

            if ch == '\\' {
                let next = cursor + ch.len_utf8();
                if source[next..]
                    .chars()
                    .next()
                    .is_some_and(|next| matches!(next, '@' | '/' | '＠' | '／'))
                {
                    cursor = next + source[next..].chars().next().map_or(0, char::len_utf8);
                    continue;
                }
            }

            let starts_ps =
                source[cursor..].starts_with("<ps") && boundary_after_ps(source, cursor);
            if ch == '<'
                && !starts_ps
                && let Some(end) = other_tag_end(source, cursor)
            {
                cursor = end + 1;
                continue;
            }

            let parsed = if starts_ps {
                self.parse_span(source, cursor, base, diagnostics)
            } else if matches!(ch, '@' | '＠') && boundary_before(source, cursor) {
                self.parse_reference_or_route(source, cursor, base, diagnostics)
            } else if matches!(ch, '/' | '／') && boundary_before(source, cursor) {
                self.parse_action(source, cursor, base, diagnostics)
            } else {
                None
            };

            if let Some((end, directive)) = parsed {
                push_text(source, base, text_start, cursor, &mut segments);
                segments.push(Segment::Directive(DirectiveSegment {
                    span: SourceSpan::new(base + cursor, base + end),
                    source: source[cursor..end].to_string(),
                    directive,
                }));
                cursor = end;
                text_start = end;
            } else {
                // A `<ps ...>` header that failed as a whole stays inert as a whole.
                // Do not resume scanning inside it and accidentally promote one of its
                // references as an independent point directive.
                if starts_ps && let Some(end) = find_tag_end(source, cursor + 3) {
                    cursor = end + 1;
                    continue;
                }
                cursor += ch.len_utf8();
            }
        }
        push_text(source, base, text_start, source.len(), &mut segments);
        segments
    }

    fn parse_reference_or_route(
        &self,
        source: &str,
        start: usize,
        base: usize,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<(usize, Directive)> {
        let parsed = match parse_reference(source, start) {
            Ok(reference) => reference,
            Err(error) => {
                if looks_qualified(source, start) {
                    diagnostics.push(error.at(base));
                }
                return None;
            }
        };
        let hinted_end = island_hint_end(source, start);
        if contains_bidi(&source[start..hinted_end]) {
            diagnostics.push(ParseFailure::bidi(start, hinted_end).at(base));
            return None;
        }
        if !self.reference_is_live(&parsed.value) {
            return None;
        }

        let mut steps = vec![RouteStep {
            reference: parsed.value.clone(),
            limits: Vec::new(),
        }];
        let mut end = parsed.end;
        let mut is_route = false;

        if let Some((after, limits)) = parse_named_arguments(source, end, "limit") {
            steps[0].limits = limits;
            end = after;
            is_route = true;
        }

        let mut terminal = None;
        loop {
            let checkpoint = end;
            let after_space = skip_space(source, end);
            let Some(after_else) = keyword(source, after_space, "else") else {
                break;
            };
            let next = skip_space(source, after_else);
            if let Some(after) = keyword(source, next, "ask") {
                terminal = Some(RouteTerminal::Ask);
                end = after;
                is_route = true;
                break;
            }
            if let Some(after) = keyword(source, next, "fail") {
                terminal = Some(RouteTerminal::Fail);
                end = after;
                is_route = true;
                break;
            }
            let Ok(next_ref) = parse_reference(source, next) else {
                end = checkpoint;
                break;
            };
            if !self.reference_is_live(&next_ref.value) {
                end = checkpoint;
                break;
            }
            let mut step = RouteStep {
                reference: next_ref.value,
                limits: Vec::new(),
            };
            end = next_ref.end;
            if let Some((after, limits)) = parse_named_arguments(source, end, "limit") {
                step.limits = limits;
                end = after;
            }
            steps.push(step);
            is_route = true;
        }

        Some((
            end,
            if is_route {
                Directive::Route(Route { steps, terminal })
            } else {
                Directive::Reference(parsed.value)
            },
        ))
    }

    fn parse_action(
        &self,
        source: &str,
        start: usize,
        base: usize,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<(usize, Directive)> {
        let parsed = parse_action(source, start).ok()?;
        let hinted_end = island_hint_end(source, start);
        if contains_bidi(&source[start..hinted_end]) {
            diagnostics.push(ParseFailure::bidi(start, hinted_end).at(base));
            return None;
        }
        if !self.actions.contains(&normalize(&parsed.value.name)) {
            return None;
        }
        Some((parsed.end, Directive::Action(parsed.value)))
    }

    fn parse_span(
        &self,
        source: &str,
        start: usize,
        base: usize,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<(usize, Directive)> {
        let header_end = find_tag_end(source, start + 3)?;
        let header = &source[start + 3..header_end];
        if contains_bidi(header) {
            diagnostics.push(ParseFailure::bidi(start, header_end + 1).at(base));
            if self.is_declared_authoring_header(header)
                && line_tail_is_empty(source, header_end + 1)
            {
                return Some((
                    header_end + 1,
                    Directive::InvalidAuthoringSegment {
                        header: header.trim().to_string(),
                    },
                ));
            }
            return None;
        }
        let body_start = header_end + 1;
        let Some(close) = find_span_close(source, body_start) else {
            if line_tail_is_empty(source, body_start) {
                if let Ok(reference) = parse_reference(header.trim(), 0)
                    && reference.end == header.trim().len()
                    && reference
                        .value
                        .namespace
                        .as_ref()
                        .is_some_and(|namespace| self.authoring_namespaces.contains(namespace))
                {
                    return Some((
                        body_start,
                        Directive::AuthoringSegment {
                            reference: reference.value,
                        },
                    ));
                }
                if self.is_declared_authoring_header(header) {
                    diagnostics.push(Diagnostic {
                        code: DiagnosticCode::SyntaxInvalid,
                        message: "malformed declared Prompt Syntax authoring segment".into(),
                        span: SourceSpan::new(base + start, base + body_start),
                    });
                    return Some((
                        body_start,
                        Directive::InvalidAuthoringSegment {
                            header: header.trim().to_string(),
                        },
                    ));
                }
            }
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::UnclosedSpan,
                message: "Prompt Syntax span has no closing </ps> tag".into(),
                span: SourceSpan::new(base + start, base + source.len()),
            });
            return None;
        };
        let body = &source[body_start..close];
        let parsed_header = Self::parse_span_header(header).unwrap_or_else(|| {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::SyntaxInvalid,
                message: "malformed Prompt Syntax span header".into(),
                span: SourceSpan::new(base + start, base + body_start),
            });
            SpanHeader {
                raw: header.trim().to_string(),
                references: Vec::new(),
                attributes: Vec::new(),
            }
        });
        let segments = self.parse_region(body, base + body_start, diagnostics, false);
        let end = close + "</ps>".len();
        Some((
            end,
            Directive::Span {
                header: parsed_header,
                segments,
            },
        ))
    }

    fn is_declared_authoring_header(&self, header: &str) -> bool {
        let trimmed = header.trim();
        let Some(after_sigil) = trimmed
            .strip_prefix('@')
            .or_else(|| trimmed.strip_prefix('＠'))
        else {
            return false;
        };
        let Some((namespace, _)) = after_sigil.split_once(':') else {
            return false;
        };
        self.authoring_namespaces.contains(&normalize(namespace))
    }

    fn parse_span_header(header: &str) -> Option<SpanHeader> {
        let mut references = Vec::new();
        let mut attributes = Vec::new();
        let mut cursor = 0;
        while cursor < header.len() {
            cursor = skip_space(header, cursor);
            if cursor == header.len() {
                break;
            }
            if header[cursor..]
                .chars()
                .next()
                .is_some_and(|ch| matches!(ch, '@' | '＠'))
            {
                let reference = parse_reference(header, cursor).ok()?;
                references.push(reference.value);
                cursor = reference.end;
                continue;
            }
            let (key, after_key) = parse_name_token(header, cursor)?;
            cursor = skip_space(header, after_key);
            if !header[cursor..].starts_with('=') {
                return None;
            }
            cursor = skip_space(header, cursor + 1);
            let quote = header[cursor..].chars().next()?;
            if !matches!(quote, '\'' | '"') {
                return None;
            }
            let value_start = cursor;
            cursor += quote.len_utf8();
            let mut escaped = false;
            let mut close = None;
            while cursor < header.len() {
                let ch = header[cursor..].chars().next()?;
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == quote {
                    close = Some(cursor + ch.len_utf8());
                    break;
                }
                cursor += ch.len_utf8();
            }
            let end = close?;
            attributes.push(SpanAttribute {
                key: normalize(&key),
                value: parse_scalar(&header[value_start..end])?,
            });
            cursor = end;
        }
        Some(SpanHeader {
            raw: header.trim().to_string(),
            references,
            attributes,
        })
    }

    fn reference_is_live(&self, reference: &Reference) -> bool {
        reference.namespace.is_some() || self.entities.contains(&reference.lookup_name())
    }
}

#[derive(Debug)]
struct Parsed<T> {
    value: T,
    end: usize,
}

#[derive(Debug)]
struct ParseFailure {
    code: DiagnosticCode,
    message: &'static str,
    start: usize,
    end: usize,
}

impl ParseFailure {
    const fn syntax(start: usize, end: usize) -> Self {
        Self {
            code: DiagnosticCode::SyntaxInvalid,
            message: "malformed Prompt Syntax island",
            start,
            end,
        }
    }

    const fn bidi(start: usize, end: usize) -> Self {
        Self {
            code: DiagnosticCode::BidiControl,
            message: "bidirectional control character inside Prompt Syntax island",
            start,
            end,
        }
    }

    fn at(self, base: usize) -> Diagnostic {
        Diagnostic {
            code: self.code,
            message: self.message.into(),
            span: SourceSpan::new(base + self.start, base + self.end),
        }
    }
}

fn parse_reference(source: &str, start: usize) -> Result<Parsed<Reference>, ParseFailure> {
    let first = source[start..].chars().next();
    if !first.is_some_and(|ch| matches!(ch, '@' | '＠')) {
        return Err(ParseFailure::syntax(start, start.saturating_add(1)));
    }
    let mut cursor = start + first.map_or(1, char::len_utf8);
    let first_name = parse_name_token(source, cursor)
        .ok_or_else(|| ParseFailure::syntax(start, island_hint_end(source, start)))?;
    cursor = first_name.1;
    let (namespace, name) = if source[cursor..].starts_with(':') {
        cursor += 1;
        let parsed = parse_name_token(source, cursor)
            .ok_or_else(|| ParseFailure::syntax(start, island_hint_end(source, start)))?;
        cursor = parsed.1;
        (Some(first_name.0), parsed.0)
    } else {
        (None, first_name.0)
    };

    let version = if source[cursor..].starts_with('@') {
        cursor += 1;
        let (version, end) = parse_version(source, cursor)
            .ok_or_else(|| ParseFailure::syntax(start, island_hint_end(source, start)))?;
        cursor = end;
        Some(version)
    } else {
        None
    };
    let strict = source[cursor..].starts_with('!');
    if strict {
        cursor += 1;
    }
    let (arguments, end) = if source[cursor..].starts_with('(') {
        parse_arguments(source, cursor)
            .ok_or_else(|| ParseFailure::syntax(start, island_hint_end(source, start)))?
    } else {
        (Vec::new(), cursor)
    };

    Ok(Parsed {
        value: Reference {
            namespace: namespace.map(|value| normalize(&value)),
            name: normalize(&name),
            version,
            strict,
            arguments,
        },
        end,
    })
}

fn parse_action(source: &str, start: usize) -> Result<Parsed<Action>, ParseFailure> {
    let first = source[start..].chars().next();
    if !first.is_some_and(|ch| matches!(ch, '/' | '／')) {
        return Err(ParseFailure::syntax(start, start.saturating_add(1)));
    }
    let cursor = start + first.map_or(1, char::len_utf8);
    let (name, mut end) = parse_name_token(source, cursor)
        .ok_or_else(|| ParseFailure::syntax(start, island_hint_end(source, start)))?;
    let (arguments, parsed_end) = if source[end..].starts_with('(') {
        parse_arguments(source, end)
            .ok_or_else(|| ParseFailure::syntax(start, island_hint_end(source, start)))?
    } else {
        (Vec::new(), end)
    };
    end = parsed_end;
    Ok(Parsed {
        value: Action {
            name: normalize(&name),
            arguments,
        },
        end,
    })
}

fn parse_name_token(source: &str, start: usize) -> Option<(String, usize)> {
    let mut cursor = start;
    let mut segment_start = true;
    while cursor < source.len() {
        let ch = source[cursor..].chars().next()?;
        if matches!(ch, '.' | '/') {
            if segment_start {
                break;
            }
            segment_start = true;
            cursor += ch.len_utf8();
            continue;
        }
        let allowed = if segment_start {
            unicode_ident::is_xid_start(ch) || ch == '_' || ch.is_ascii_digit()
        } else {
            unicode_ident::is_xid_continue(ch) || matches!(ch, '-' | '_')
        };
        if !allowed {
            break;
        }
        segment_start = false;
        cursor += ch.len_utf8();
    }
    (cursor > start && !segment_start).then(|| (source[start..cursor].to_string(), cursor))
}

fn parse_version(source: &str, start: usize) -> Option<(String, usize)> {
    let mut cursor = start;
    while cursor < source.len() {
        let ch = source[cursor..].chars().next()?;
        if unicode_ident::is_xid_continue(ch)
            || ch.is_ascii_digit()
            || matches!(ch, '.' | '-' | '_')
        {
            cursor += ch.len_utf8();
        } else {
            break;
        }
    }
    (cursor > start).then(|| (source[start..cursor].to_string(), cursor))
}

fn parse_arguments(source: &str, open: usize) -> Option<(Vec<Argument>, usize)> {
    let close = balanced_close(source, open, '(', ')')?;
    let inside = &source[open + 1..close];
    let mut arguments = Vec::new();
    for raw in split_top_level(inside, ',') {
        if raw.trim().is_empty() {
            continue;
        }
        let (key, value) = split_key_value(raw)?;
        let key = key.trim();
        if key.is_empty()
            || !key.chars().enumerate().all(|(index, ch)| {
                if index == 0 {
                    unicode_ident::is_xid_start(ch) || ch == '_'
                } else {
                    unicode_ident::is_xid_continue(ch) || ch == '_'
                }
            })
        {
            return None;
        }
        arguments.push(Argument {
            key: normalize(key),
            value: parse_scalar(value.trim())?,
        });
    }
    Some((arguments, close + 1))
}

fn parse_named_arguments(source: &str, start: usize, name: &str) -> Option<(usize, Vec<Argument>)> {
    let cursor = skip_space(source, start);
    let after = keyword(source, cursor, name)?;
    let open = skip_space(source, after);
    if !source[open..].starts_with('(') {
        return None;
    }
    let (arguments, end) = parse_arguments(source, open)?;
    Some((end, arguments))
}

fn parse_scalar(value: &str) -> Option<Scalar> {
    if value.len() >= 2 {
        let quote = value.chars().next()?;
        if matches!(quote, '\'' | '"') && value.ends_with(quote) {
            return unescape_string(
                &value[quote.len_utf8()..value.len() - quote.len_utf8()],
                quote,
            )
            .map(Scalar::String);
        }
    }
    match value {
        "true" => Some(Scalar::Boolean(true)),
        "false" => Some(Scalar::Boolean(false)),
        "null" => Some(Scalar::Null),
        "" => None,
        raw if looks_number(raw) => Some(Scalar::Number(raw.to_string())),
        raw => Some(Scalar::Bare(raw.to_string())),
    }
}

fn unescape_string(value: &str, quote: char) -> Option<String> {
    let mut output = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        let escaped = chars.next()?;
        output.push(match escaped {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '\\' => '\\',
            other if other == quote => quote,
            other => other,
        });
    }
    Some(output)
}

fn looks_number(value: &str) -> bool {
    value.parse::<f64>().is_ok()
        || value
            .strip_prefix('$')
            .is_some_and(|amount| amount.parse::<f64>().is_ok())
        || ["ms", "s", "tok"].iter().any(|unit| {
            value
                .strip_suffix(unit)
                .is_some_and(|n| n.parse::<f64>().is_ok())
        })
}

fn split_key_value(value: &str) -> Option<(&str, &str)> {
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        match (quote, ch) {
            (Some(open), close) if open == close => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, ':') => return Some((&value[..index], &value[index + 1..])),
            _ => {}
        }
    }
    None
}

fn split_top_level(value: &str, separator: char) -> Vec<&str> {
    let mut found = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 0usize;
    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        match (quote, ch) {
            (Some(open), close) if open == close => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, '(' | '[' | '{') => depth += 1,
            (None, ')' | ']' | '}') => depth = depth.saturating_sub(1),
            (None, ch) if ch == separator && depth == 0 => {
                found.push(&value[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    found.push(&value[start..]);
    found
}

fn balanced_close(source: &str, open: usize, opener: char, closer: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (offset, ch) in source[open..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        match (quote, ch) {
            (Some(active), close) if active == close => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, current) if current == opener => depth += 1,
            (None, current) if current == closer => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn frontmatter_open_end(source: &str) -> Option<usize> {
    let end = source.find('\n')? + 1;
    (source[..end].trim_end_matches(['\r', '\n']) == "---ps").then_some(end)
}

fn closing_frontmatter(source: &str, open_end: usize) -> Option<(usize, usize)> {
    let mut cursor = open_end;
    while cursor < source.len() {
        let end = source[cursor..]
            .find('\n')
            .map_or(source.len(), |offset| cursor + offset + 1);
        let line = source[cursor..end].trim_end_matches(['\r', '\n']);
        if line == "---" {
            return Some((cursor, end));
        }
        cursor = end;
    }
    None
}

fn fenced_block_end(source: &str, start: usize) -> Option<usize> {
    if start > 0 && source.as_bytes().get(start.wrapping_sub(1)) != Some(&b'\n') {
        return None;
    }
    let line_end = source[start..]
        .find('\n')
        .map_or(source.len(), |at| start + at + 1);
    let line = source[start..line_end].trim_end_matches(['\r', '\n']);
    let indent = line.chars().take_while(|ch| *ch == ' ').count();
    if indent > 3 {
        return None;
    }
    let trimmed = &line[indent..];
    let marker = trimmed.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let width = trimmed.chars().take_while(|ch| *ch == marker).count();
    if width < 3 {
        return None;
    }
    let mut cursor = line_end;
    while cursor < source.len() {
        let next = source[cursor..]
            .find('\n')
            .map_or(source.len(), |at| cursor + at + 1);
        let candidate = source[cursor..next].trim_end_matches(['\r', '\n']).trim();
        let close_width = candidate.chars().take_while(|ch| *ch == marker).count();
        if close_width >= width && candidate[close_width..].trim().is_empty() {
            return Some(next);
        }
        cursor = next;
    }
    Some(source.len())
}

fn find_tag_end(source: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (offset, ch) in source[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        match (quote, ch) {
            (Some(active), close) if active == close => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, '>') => return Some(start + offset),
            _ => {}
        }
    }
    None
}

fn other_tag_end(source: &str, start: usize) -> Option<usize> {
    let mut cursor = start.checked_add(1)?;
    if source[cursor..].starts_with('/') {
        cursor += 1;
    }
    let first = source[cursor..].chars().next()?;
    if !first.is_alphabetic() && !matches!(first, '!' | '?') {
        return None;
    }
    find_tag_end(source, cursor + first.len_utf8())
}

fn find_span_close(source: &str, start: usize) -> Option<usize> {
    let mut cursor = start;
    let mut depth = 1usize;
    while cursor < source.len() {
        if let Some(end) = fenced_block_end(source, cursor) {
            cursor = end;
            continue;
        }
        if source[cursor..].starts_with("<ps") && boundary_after_ps(source, cursor) {
            let end = find_tag_end(source, cursor + 3)?;
            depth += 1;
            cursor = end + 1;
            continue;
        }
        if source[cursor..].starts_with("</ps>") {
            depth -= 1;
            if depth == 0 {
                return Some(cursor);
            }
            cursor += "</ps>".len();
            continue;
        }
        cursor += source[cursor..].chars().next()?.len_utf8();
    }
    None
}

fn boundary_after_ps(source: &str, start: usize) -> bool {
    source[start + 3..]
        .chars()
        .next()
        .is_some_and(|ch| ch == '>' || ch.is_whitespace())
}

fn line_tail_is_empty(source: &str, start: usize) -> bool {
    let end = source[start..]
        .find(['\r', '\n'])
        .map_or(source.len(), |offset| start + offset);
    source[start..end].trim().is_empty()
}

fn boundary_before(source: &str, start: usize) -> bool {
    let Some(previous) = source[..start].chars().next_back() else {
        return true;
    };
    !previous.is_ascii_alphanumeric() && !matches!(previous, '_' | '.' | '-' | '\\')
}

fn keyword(source: &str, start: usize, expected: &str) -> Option<usize> {
    let tail = source.get(start..)?;
    let matched = tail.get(..expected.len())?;
    if !matched.eq_ignore_ascii_case(expected) {
        return None;
    }
    let end = start + expected.len();
    if source[end..]
        .chars()
        .next()
        .is_some_and(|ch| unicode_ident::is_xid_continue(ch) || ch == '_')
    {
        return None;
    }
    Some(end)
}

fn skip_space(source: &str, mut cursor: usize) -> usize {
    while let Some(ch) = source[cursor..].chars().next() {
        if !ch.is_whitespace() {
            break;
        }
        cursor += ch.len_utf8();
    }
    cursor
}

fn looks_qualified(source: &str, start: usize) -> bool {
    let end = island_hint_end(source, start);
    source[start..end].contains(':')
}

fn island_hint_end(source: &str, start: usize) -> usize {
    source[start..]
        .find(char::is_whitespace)
        .map_or(source.len(), |offset| start + offset)
}

fn contains_bidi(value: &str) -> bool {
    value.chars().any(|ch| {
        matches!(
            ch,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
    })
}

fn normalize(value: &str) -> String {
    value.nfc().collect()
}

fn push_text(source: &str, base: usize, start: usize, end: usize, output: &mut Vec<Segment>) {
    if start < end {
        output.push(text_segment(source, base, start, end));
    }
}

fn text_segment(source: &str, base: usize, start: usize, end: usize) -> Segment {
    Segment::Text(TextSegment {
        span: SourceSpan::new(base + start, base + end),
        text: source[start..end].to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_control_and_data_planes_losslessly() {
        let source = "@opus Summarize @file:q3-report.md /concise";
        let parsed = Parser::new().entity("opus").action("concise").parse(source);
        assert_eq!(parsed.round_trip(), source);
        assert_eq!(parsed.data_plane(), " Summarize  ");
        assert_eq!(parsed.directives().count(), 3);
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn unknown_bare_names_emails_paths_escapes_and_fences_are_inert() {
        let source = "mail a@b.com @unknown /usr/bin \\@model:x\n```\n@model:y\n```";
        let parsed = Parser::new().parse(source);
        assert_eq!(parsed.data_plane(), source);
        assert_eq!(parsed.directives().count(), 0);
    }

    #[test]
    fn parses_full_width_sigils_and_unicode_names() {
        let parsed = Parser::new()
            .action("បកប្រែ")
            .parse("＠file:របាយការណ៍.md ／បកប្រែ");
        assert_eq!(parsed.directives().count(), 2);
    }

    #[test]
    fn parses_arguments_with_escaped_quotes_and_commas() {
        let parsed = Parser::new()
            .parse(r#"@agency:items.add(title: "Fix \"quoted\", text", status: active)"#);
        let Segment::Directive(segment) = &parsed.segments[0] else {
            panic!("expected directive");
        };
        let Directive::Reference(reference) = &segment.directive else {
            panic!("expected reference");
        };
        assert_eq!(reference.arguments.len(), 2);
        assert_eq!(
            reference.arguments[0].value,
            Scalar::String("Fix \"quoted\", text".into())
        );
    }

    #[test]
    fn parses_budgeted_fallback_routes() {
        let source =
            "@opus! limit(wall_time: 5s) else @model:openai/gpt-5.6@2026-07-01 else ask\nDo it";
        let parsed = Parser::new().entity("opus").parse(source);
        let Segment::Directive(first) = &parsed.segments[0] else {
            panic!("expected route");
        };
        let Directive::Route(route) = &first.directive else {
            panic!("expected route");
        };
        assert_eq!(route.steps.len(), 2);
        assert_eq!(route.terminal, Some(RouteTerminal::Ask));
        assert_eq!(parsed.data_plane(), "\nDo it");
    }

    #[test]
    fn span_envelope_is_control_but_authored_inner_text_remains_data() {
        let source = "Before <ps @file:glossary.md>use @file:terms.md here</ps> after";
        let parsed = Parser::new().parse(source);
        assert_eq!(parsed.data_plane(), "Before use  here after");
        assert_eq!(parsed.directives().count(), 2);
        assert_eq!(parsed.round_trip(), source);
        let Segment::Directive(segment) = &parsed.segments[1] else {
            panic!("expected span");
        };
        let Directive::Span { header, .. } = &segment.directive else {
            panic!("expected span");
        };
        assert_eq!(header.references.len(), 1);
    }

    #[test]
    fn parses_canonical_span_attributes() {
        let parsed = Parser::new()
            .parse(r#"<ps context="file:workspace/acme/9fc2" fill="strict">Summarize</ps>"#);
        let Segment::Directive(segment) = &parsed.segments[0] else {
            panic!("expected span");
        };
        let Directive::Span { header, .. } = &segment.directive else {
            panic!("expected span");
        };
        assert_eq!(header.attributes.len(), 2);
        assert_eq!(header.attributes[0].key, "context");
        assert_eq!(header.attributes[1].value, Scalar::String("strict".into()));
    }

    #[test]
    fn strict_frontmatter_is_control_plane() {
        let source = "---ps\nversion: \"0.2\"\n---\nSummarize.";
        let parsed = Parser::new().parse(source);
        assert_eq!(parsed.data_plane(), "Summarize.");
        assert_eq!(parsed.directives().count(), 1);
    }

    #[test]
    fn empty_frontmatter_without_a_final_newline_is_valid() {
        let parsed = Parser::new().parse("---ps\n---");
        let Segment::Directive(segment) = &parsed.segments[0] else {
            panic!("expected frontmatter");
        };
        assert!(matches!(
            &segment.directive,
            Directive::Frontmatter { body } if body.is_empty()
        ));
    }

    #[test]
    fn frontmatter_accepts_windows_line_endings() {
        let parsed = Parser::new().parse("---ps\r\nversion: \"0.2\"\r\n---\r\nBody");
        assert_eq!(parsed.data_plane(), "Body");
        assert_eq!(parsed.directives().count(), 1);
    }

    #[test]
    fn declared_single_line_authoring_segment_is_control_plane() {
        let source =
            "Before\n<ps @agency:items.state(id: \"item-869382d3\", status: \"active\")>\nAfter";
        let parsed = Parser::new().authoring_namespace("agency").parse(source);
        assert_eq!(parsed.data_plane(), "Before\n\nAfter");
        assert_eq!(parsed.directives().count(), 1);
        assert!(matches!(
            &parsed.segments[1],
            Segment::Directive(DirectiveSegment {
                directive: Directive::AuthoringSegment { .. },
                ..
            })
        ));
    }

    #[test]
    fn undeclared_single_line_tag_stays_inert() {
        let source = "<ps @agency:items.state(id: \"x\", status: \"active\")>";
        let parsed = Parser::new().parse(source);
        assert_eq!(parsed.data_plane(), source);
        assert_eq!(parsed.directives().count(), 0);
        assert_eq!(parsed.diagnostics[0].code, DiagnosticCode::UnclosedSpan);
    }

    #[test]
    fn bidi_control_rejects_the_whole_island() {
        let source = "@model:openai/gpt\u{202e}-5";
        let parsed = Parser::new().parse(source);
        assert_eq!(parsed.data_plane(), source);
        assert_eq!(parsed.directives().count(), 0);
        assert_eq!(parsed.diagnostics[0].code, DiagnosticCode::BidiControl);
    }

    #[test]
    fn malformed_qualified_reference_is_text_with_a_diagnostic() {
        let source = "Use @model: now";
        let parsed = Parser::new().parse(source);
        assert_eq!(parsed.data_plane(), source);
        assert_eq!(parsed.diagnostics[0].code, DiagnosticCode::SyntaxInvalid);
    }

    #[test]
    fn arbitrary_unicode_input_round_trips_without_panicking() {
        const ALPHABET: [char; 22] = [
            'a', '5', '@', '＠', '/', '／', '<', '>', '(', ')', ':', ',', '\\', '\n', ' ', '"',
            '\'', '`', '~', 'ក', '報', '\u{202e}',
        ];
        let parser = Parser::new()
            .entity("a")
            .action("a")
            .authoring_namespace("a");
        let mut state = 0x5eed_u64;
        for _ in 0..2_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let len = usize::try_from(state % 48).expect("small length");
            let mut source = String::new();
            for _ in 0..len {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                let index = usize::try_from(state % ALPHABET.len() as u64).expect("small index");
                source.push(ALPHABET[index]);
            }
            let parsed = parser.parse(&source);
            assert_eq!(parsed.round_trip(), source);
            for directive in parsed.directives() {
                assert!(source.is_char_boundary(directive.span.start));
                assert!(source.is_char_boundary(directive.span.end));
                assert_eq!(
                    &source[directive.span.start..directive.span.end],
                    directive.source
                );
            }
        }
    }
}
