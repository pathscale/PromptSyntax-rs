#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path};

use jsonschema::PatternOptions;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const SUPPORTED_FORMAT: &str = "0.1-draft";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementsDocument {
    pub format_version: String,
    pub spec: SpecBaseline,
    pub requirements: Vec<Requirement>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpecBaseline {
    pub path: String,
    pub commit: String,
    pub declared_version: String,
    pub baseline_status: BaselineStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BaselineStatus {
    WorkingDraft,
    Frozen,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub id: String,
    pub layer: Layer,
    pub normative_status: NormativeStatus,
    pub strength: Strength,
    pub source: RequirementSource,
    pub statement: String,
    pub testability: Testability,
    pub targets: Vec<Target>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementSource {
    pub path: String,
    pub section: String,
    pub candidate_anchor: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    Meta,
    Core,
    Capabilities,
    Execution,
    Trace,
}

impl Layer {
    const fn id_component(self) -> &'static str {
        match self {
            Self::Meta => "META",
            Self::Core => "CORE",
            Self::Capabilities => "CAP",
            Self::Execution => "EXEC",
            Self::Trace => "TRACE",
        }
    }

    const fn report_name(self) -> &'static str {
        match self {
            Self::Meta => "meta",
            Self::Core => "core",
            Self::Capabilities => "capabilities",
            Self::Execution => "execution",
            Self::Trace => "trace",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NormativeStatus {
    Settled,
    Proposed,
    Open,
}

impl NormativeStatus {
    const fn report_name(self) -> &'static str {
        match self {
            Self::Settled => "settled",
            Self::Proposed => "proposed",
            Self::Open => "open",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Strength {
    Must,
    MustNot,
    Should,
    ShouldNot,
    May,
    Invariant,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Testability {
    Automatic,
    Manual,
    Profile,
    Blocked,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    Corpus,
    CoreParser,
    CapabilityDocument,
    ExecutionResolver,
    TraceDocument,
    TraceProducer,
    InteractiveUi,
    Documentation,
    OversightProfile,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    pub code: String,
    pub pointer: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CheckReport {
    pub format_version: String,
    pub conformant: bool,
    pub requirement_count: usize,
    pub counts: BTreeMap<String, usize>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ValidationReport {
    pub kind: String,
    pub conformant: bool,
    pub diagnostics: Vec<Diagnostic>,
}

impl ValidationReport {
    #[must_use]
    pub fn invalid_input(kind: &str, code: &str, message: impl Into<String>) -> Self {
        Self {
            kind: kind.to_owned(),
            conformant: false,
            diagnostics: vec![Diagnostic {
                code: code.to_owned(),
                pointer: String::new(),
                message: message.into(),
            }],
        }
    }
}

impl CheckReport {
    #[must_use]
    pub fn invalid_input(code: &str, message: impl Into<String>) -> Self {
        Self {
            format_version: SUPPORTED_FORMAT.to_owned(),
            conformant: false,
            requirement_count: 0,
            counts: BTreeMap::new(),
            diagnostics: vec![Diagnostic {
                code: code.to_owned(),
                pointer: String::new(),
                message: message.into(),
            }],
        }
    }
}

#[must_use]
pub fn check_requirements_json(input: &[u8]) -> CheckReport {
    match serde_json::from_slice::<RequirementsDocument>(input) {
        Ok(document) => check_requirements(&document),
        Err(error) => CheckReport::invalid_input("CORPUS_JSON_INVALID", error.to_string()),
    }
}

#[must_use]
pub fn check_schema_json(input: &[u8]) -> ValidationReport {
    let schema = match serde_json::from_slice::<Value>(input) {
        Ok(schema) => schema,
        Err(error) => {
            return ValidationReport::invalid_input(
                "schema",
                "SCHEMA_JSON_INVALID",
                error.to_string(),
            );
        }
    };

    match jsonschema::meta::validate(&schema) {
        Ok(()) => ValidationReport {
            kind: "schema".to_owned(),
            conformant: true,
            diagnostics: Vec::new(),
        },
        Err(error) => {
            ValidationReport::invalid_input("schema", "SCHEMA_META_INVALID", error.to_string())
        }
    }
}

#[must_use]
pub fn validate_instance_json(schema_input: &[u8], instance_input: &[u8]) -> ValidationReport {
    let schema = match serde_json::from_slice::<Value>(schema_input) {
        Ok(schema) => schema,
        Err(error) => {
            return ValidationReport::invalid_input(
                "instance",
                "SCHEMA_JSON_INVALID",
                error.to_string(),
            );
        }
    };
    let instance = match serde_json::from_slice::<Value>(instance_input) {
        Ok(instance) => instance,
        Err(error) => {
            return ValidationReport::invalid_input(
                "instance",
                "INSTANCE_JSON_INVALID",
                error.to_string(),
            );
        }
    };

    if let Err(error) = jsonschema::meta::validate(&schema) {
        return ValidationReport::invalid_input(
            "instance",
            "SCHEMA_META_INVALID",
            error.to_string(),
        );
    }

    let validator = match jsonschema::draft202012::options()
        .with_pattern_options(PatternOptions::regex())
        .should_validate_formats(true)
        .build(&schema)
    {
        Ok(validator) => validator,
        Err(error) => {
            return ValidationReport::invalid_input(
                "instance",
                "SCHEMA_BUILD_FAILED",
                error.to_string(),
            );
        }
    };

    let mut diagnostics = validator
        .iter_errors(&instance)
        .map(|error| Diagnostic {
            code: "SCHEMA_INSTANCE_INVALID".to_owned(),
            pointer: error.instance_path().to_string(),
            message: error.to_string(),
        })
        .collect::<Vec<_>>();
    diagnostics.sort_by(|left, right| {
        left.pointer
            .cmp(&right.pointer)
            .then_with(|| left.message.cmp(&right.message))
    });

    ValidationReport {
        kind: "instance".to_owned(),
        conformant: diagnostics.is_empty(),
        diagnostics,
    }
}

#[must_use]
pub fn check_requirements(document: &RequirementsDocument) -> CheckReport {
    let mut diagnostics = Vec::new();

    if document.format_version != SUPPORTED_FORMAT {
        push_diagnostic(
            &mut diagnostics,
            "CORPUS_FORMAT_UNSUPPORTED",
            "/format_version",
            format!(
                "expected format {SUPPORTED_FORMAT}, got {}",
                document.format_version
            ),
        );
    }

    if !is_lower_hex_commit(&document.spec.commit) {
        push_diagnostic(
            &mut diagnostics,
            "CORPUS_SPEC_COMMIT_INVALID",
            "/spec/commit",
            "commit must contain exactly 40 lowercase hexadecimal characters",
        );
    }

    if !is_safe_relative_path(&document.spec.path) {
        push_diagnostic(
            &mut diagnostics,
            "CORPUS_SOURCE_PATH_INVALID",
            "/spec/path",
            "spec path must be a non-empty relative path without parent traversal",
        );
    }

    if document.requirements.is_empty() {
        push_diagnostic(
            &mut diagnostics,
            "CORPUS_REQUIREMENTS_EMPTY",
            "/requirements",
            "at least one requirement is required",
        );
    }

    let mut ids = HashSet::new();
    for (index, requirement) in document.requirements.iter().enumerate() {
        check_requirement(requirement, index, &mut ids, &mut diagnostics);
    }

    let mut counts = BTreeMap::new();
    for requirement in &document.requirements {
        increment_count(
            &mut counts,
            format!("layer:{}", requirement.layer.report_name()),
        );
        increment_count(
            &mut counts,
            format!("status:{}", requirement.normative_status.report_name()),
        );
    }

    diagnostics.sort_by(|left, right| {
        left.pointer
            .cmp(&right.pointer)
            .then_with(|| left.code.cmp(&right.code))
    });

    CheckReport {
        format_version: document.format_version.clone(),
        conformant: diagnostics.is_empty(),
        requirement_count: document.requirements.len(),
        counts,
        diagnostics,
    }
}

fn check_requirement(
    requirement: &Requirement,
    index: usize,
    ids: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let base = format!("/requirements/{index}");
    let expected_prefix = format!("PS-{}-", requirement.layer.id_component());

    if !is_requirement_id(&requirement.id) || !requirement.id.starts_with(&expected_prefix) {
        push_diagnostic(
            diagnostics,
            "CORPUS_REQUIREMENT_ID_INVALID",
            format!("{base}/id"),
            format!("requirement id must use uppercase ASCII and start with {expected_prefix}"),
        );
    }

    if !ids.insert(requirement.id.clone()) {
        push_diagnostic(
            diagnostics,
            "CORPUS_REQUIREMENT_ID_DUPLICATE",
            format!("{base}/id"),
            format!("duplicate requirement id {}", requirement.id),
        );
    }

    if requirement.statement.trim().is_empty() {
        push_diagnostic(
            diagnostics,
            "CORPUS_REQUIREMENT_STATEMENT_EMPTY",
            format!("{base}/statement"),
            "requirement statement must not be empty",
        );
    }

    check_source(&requirement.source, &base, diagnostics);
    check_targets(&requirement.targets, &base, diagnostics);
    check_blockers(requirement, &base, diagnostics);
}

fn check_source(source: &RequirementSource, base: &str, diagnostics: &mut Vec<Diagnostic>) {
    if !is_safe_relative_path(&source.path) {
        push_diagnostic(
            diagnostics,
            "CORPUS_SOURCE_PATH_INVALID",
            format!("{base}/source/path"),
            "source path must be relative and must not contain parent traversal",
        );
    }

    if source.section.trim().is_empty() {
        push_diagnostic(
            diagnostics,
            "CORPUS_SOURCE_SECTION_EMPTY",
            format!("{base}/source/section"),
            "source section must not be empty",
        );
    }

    if !is_candidate_anchor(&source.candidate_anchor) {
        push_diagnostic(
            diagnostics,
            "CORPUS_SOURCE_ANCHOR_INVALID",
            format!("{base}/source/candidate_anchor"),
            "candidate anchor must use lowercase ASCII letters, digits, and hyphens",
        );
    }
}

fn check_targets(targets: &[Target], base: &str, diagnostics: &mut Vec<Diagnostic>) {
    if targets.is_empty() {
        push_diagnostic(
            diagnostics,
            "CORPUS_TARGETS_EMPTY",
            format!("{base}/targets"),
            "at least one target is required",
        );
    }

    let unique_targets = targets.iter().copied().collect::<HashSet<_>>();
    if unique_targets.len() != targets.len() {
        push_diagnostic(
            diagnostics,
            "CORPUS_TARGET_DUPLICATE",
            format!("{base}/targets"),
            "targets must be unique",
        );
    }
}

fn check_blockers(requirement: &Requirement, base: &str, diagnostics: &mut Vec<Diagnostic>) {
    for (blocker_index, blocker) in requirement.blocked_by.iter().enumerate() {
        if !is_blocker_id(blocker) {
            push_diagnostic(
                diagnostics,
                "CORPUS_BLOCKER_ID_INVALID",
                format!("{base}/blocked_by/{blocker_index}"),
                "blocker id must use the form A01",
            );
        }
    }

    let unique_blockers = requirement.blocked_by.iter().collect::<HashSet<_>>();
    if unique_blockers.len() != requirement.blocked_by.len() {
        push_diagnostic(
            diagnostics,
            "CORPUS_BLOCKER_DUPLICATE",
            format!("{base}/blocked_by"),
            "blocker ids must be unique",
        );
    }

    if requirement.testability == Testability::Blocked && requirement.blocked_by.is_empty() {
        push_diagnostic(
            diagnostics,
            "CORPUS_BLOCKED_WITHOUT_REASON",
            format!("{base}/blocked_by"),
            "a blocked requirement must identify at least one audit blocker",
        );
    }

    if requirement.testability != Testability::Blocked && !requirement.blocked_by.is_empty() {
        push_diagnostic(
            diagnostics,
            "CORPUS_BLOCKER_ON_UNBLOCKED_REQUIREMENT",
            format!("{base}/blocked_by"),
            "only blocked requirements may declare audit blockers",
        );
    }
}

fn increment_count(counts: &mut BTreeMap<String, usize>, key: String) {
    *counts.entry(key).or_default() += 1;
}

fn push_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    code: &str,
    pointer: impl Into<String>,
    message: impl Into<String>,
) {
    diagnostics.push(Diagnostic {
        code: code.to_owned(),
        pointer: pointer.into(),
        message: message.into(),
    });
}

fn is_lower_hex_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_requirement_id(value: &str) -> bool {
    value.starts_with("PS-")
        && value.len() > 3
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.ends_with('-')
        && !value.contains("--")
}

fn is_candidate_anchor(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}

fn is_blocker_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 3 && bytes[0] == b'A' && bytes[1].is_ascii_digit() && bytes[2].is_ascii_digit()
}

fn is_safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_requirement(id: &str) -> Requirement {
        Requirement {
            id: id.to_owned(),
            layer: Layer::Core,
            normative_status: NormativeStatus::Settled,
            strength: Strength::Must,
            source: RequirementSource {
                path: "spec/SPEC.md".to_owned(),
                section: "13".to_owned(),
                candidate_anchor: "ps-core-test-001".to_owned(),
            },
            statement: "The behavior is explicit.".to_owned(),
            testability: Testability::Automatic,
            targets: vec![Target::CoreParser],
            blocked_by: Vec::new(),
            notes: None,
        }
    }

    fn valid_document() -> RequirementsDocument {
        RequirementsDocument {
            format_version: SUPPORTED_FORMAT.to_owned(),
            spec: SpecBaseline {
                path: "spec/SPEC.md".to_owned(),
                commit: "a".repeat(40),
                declared_version: "0.3-draft".to_owned(),
                baseline_status: BaselineStatus::WorkingDraft,
            },
            requirements: vec![valid_requirement("PS-CORE-TEST-001")],
        }
    }

    #[test]
    fn accepts_valid_document() {
        let report = check_requirements(&valid_document());
        assert!(report.conformant, "{:?}", report.diagnostics);
        assert_eq!(report.requirement_count, 1);
        assert_eq!(report.counts["layer:core"], 1);
        assert_eq!(report.counts["status:settled"], 1);
    }

    #[test]
    fn rejects_duplicate_ids() {
        let mut document = valid_document();
        document
            .requirements
            .push(valid_requirement("PS-CORE-TEST-001"));
        let report = check_requirements(&document);
        assert!(!report.conformant);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "CORPUS_REQUIREMENT_ID_DUPLICATE")
        );
    }

    #[test]
    fn rejects_id_that_disagrees_with_layer() {
        let mut document = valid_document();
        document.requirements[0].id = "PS-TRACE-TEST-001".to_owned();
        let report = check_requirements(&document);
        assert!(!report.conformant);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "CORPUS_REQUIREMENT_ID_INVALID")
        );
    }

    #[test]
    fn blocked_requirement_names_audit_finding() {
        let mut document = valid_document();
        document.requirements[0].testability = Testability::Blocked;
        let report = check_requirements(&document);
        assert!(!report.conformant);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "CORPUS_BLOCKED_WITHOUT_REASON")
        );
    }

    #[test]
    fn rejects_unknown_json_fields() {
        let json = br#"{
          "format_version": "0.1-draft",
          "spec": {
            "path": "spec/SPEC.md",
            "commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "declared_version": "draft",
            "baseline_status": "working-draft",
            "unexpected": true
          },
          "requirements": []
        }"#;
        let report = check_requirements_json(json);
        assert!(!report.conformant);
        assert_eq!(report.diagnostics[0].code, "CORPUS_JSON_INVALID");
    }

    #[test]
    fn validates_schema_and_instance() {
        let schema = br#"{
          "$schema": "https://json-schema.org/draft/2020-12/schema",
          "type": "object",
          "additionalProperties": false,
          "required": ["value"],
          "properties": { "value": { "type": "integer", "minimum": 0 } }
        }"#;
        assert!(check_schema_json(schema).conformant);
        assert!(validate_instance_json(schema, br#"{"value": 1}"#).conformant);

        let invalid = validate_instance_json(schema, br#"{"value": -1}"#);
        assert!(!invalid.conformant);
        assert_eq!(invalid.diagnostics[0].code, "SCHEMA_INSTANCE_INVALID");
        assert_eq!(invalid.diagnostics[0].pointer, "/value");
    }
}
