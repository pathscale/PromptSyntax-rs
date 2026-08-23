//! Deterministic PS/Trace production from independently observable execution facts.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const INPUT_FORMAT: &str = "0.1-draft";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceProducerInput {
    format_version: String,
    turn: String,
    tier: String,
    mode: String,
    content_mode: String,
    venue: Venue,
    coverage: Coverage,
    inferences: Vec<InferenceInput>,
    boundaries: Vec<BoundaryInput>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Venue {
    id: String,
    capability_document: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    vendor_profile: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Coverage {
    routing: String,
    assembly: String,
    inline_threshold_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InferenceInput {
    id: String,
    kind: String,
    compiled_request_utf8: String,
    routing: RoutingInput,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoutingInput {
    requested: String,
    policy: String,
    route: Vec<RouteStep>,
    attempts: Vec<Attempt>,
    non_entity_fill: Vec<Value>,
    refusal: Option<RefusalFact>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteStep {
    #[serde(rename = "ref")]
    reference: String,
    canonical: String,
    rule: String,
    ambiguity_surfaced: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Attempt {
    #[serde(rename = "ref")]
    reference: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    bound: Option<String>,
    outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasons: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    measured: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RefusalFact {
    reason: String,
    authority: String,
    triggering_step: Option<String>,
    recourse: Value,
    policy_decision_id: Option<String>,
    policy_bundle: Option<String>,
    policy_engine: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundaryInput {
    id: String,
    tool: String,
    outcome: String,
    sent: Value,
    received: Option<Value>,
    reason: Option<String>,
    fill: Vec<Value>,
    internal: Option<String>,
    measured: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TraceProducerError {
    pub code: String,
    pub pointer: String,
    pub message: String,
}

/// Parse deterministic execution facts and emit one user-tier Prompt Trace.
///
/// # Errors
///
/// Returns a typed producer error when the input is malformed, uses an
/// unsupported execution profile, or contains contradictory execution facts.
pub fn produce_trace_json(input: &[u8]) -> Result<Value, TraceProducerError> {
    let input = serde_json::from_slice::<TraceProducerInput>(input).map_err(|error| {
        producer_error(
            "TRACE_PRODUCER_INPUT_INVALID",
            "",
            format!("producer input is invalid: {error}"),
        )
    })?;
    produce_trace(input)
}

/// Emit one Prompt Trace without receiving a transcript or expected output.
///
/// # Errors
///
/// Returns a typed producer error when the execution profile is unsupported or
/// the supplied facts cannot describe one internally consistent execution.
pub fn produce_trace(input: TraceProducerInput) -> Result<Value, TraceProducerError> {
    validate_envelope(&input)?;
    let inline_threshold_bytes = input.coverage.inline_threshold_bytes;
    let mut event_ids = HashSet::new();
    let mut events = Vec::with_capacity(input.inferences.len() + input.boundaries.len());

    for (index, inference) in input.inferences.into_iter().enumerate() {
        if !event_ids.insert(inference.id.clone()) {
            return Err(producer_error(
                "TRACE_PRODUCER_EVENT_ID_DUPLICATE",
                format!("/inferences/{index}/id"),
                "producer input repeats an event id",
            ));
        }
        events.push(produce_inference(inference, index, inline_threshold_bytes)?);
    }
    for (index, boundary) in input.boundaries.into_iter().enumerate() {
        if !event_ids.insert(boundary.id.clone()) {
            return Err(producer_error(
                "TRACE_PRODUCER_EVENT_ID_DUPLICATE",
                format!("/boundaries/{index}/id"),
                "producer input repeats an event id",
            ));
        }
        events.push(produce_boundary(boundary, index, inline_threshold_bytes)?);
    }

    Ok(json!({
        "ps_trace": "0.3-draft",
        "turn": input.turn,
        "tier": input.tier,
        "mode": input.mode,
        "content_mode": input.content_mode,
        "venue": input.venue,
        "coverage": input.coverage,
        "events": events,
    }))
}

fn validate_envelope(input: &TraceProducerInput) -> Result<(), TraceProducerError> {
    let valid = input.format_version == INPUT_FORMAT
        && input.tier == "user"
        && input.mode == "executed"
        && input.content_mode == "tiered"
        && input.coverage.routing == "complete"
        && input.coverage.assembly == "user-initiated-only"
        && input.coverage.inline_threshold_bytes >= 4096;
    if valid {
        Ok(())
    } else {
        Err(producer_error(
            "TRACE_PRODUCER_INPUT_UNSUPPORTED",
            "",
            "producer supports only the 0.1-draft user-tier executed profile",
        ))
    }
}

#[allow(clippy::too_many_lines)]
fn produce_inference(
    inference: InferenceInput,
    inference_index: usize,
    inline_threshold_bytes: usize,
) -> Result<Value, TraceProducerError> {
    let base = format!("/inferences/{inference_index}/routing");
    if inference.kind != "user-initiated" {
        return Err(producer_error(
            "TRACE_PRODUCER_INPUT_UNSUPPORTED",
            format!("/inferences/{inference_index}/kind"),
            "producer supports only user-initiated inferences",
        ));
    }
    if inference.compiled_request_utf8.len() >= inline_threshold_bytes {
        return Err(producer_error(
            "TRACE_PRODUCER_CONTENT_TOO_LARGE",
            format!("/inferences/{inference_index}/compiled_request_utf8"),
            "compiled request must be externalized at the declared inline threshold",
        ));
    }
    if inference.routing.route.is_empty() || inference.routing.attempts.is_empty() {
        return Err(producer_error(
            "TRACE_PRODUCER_ROUTE_INVALID",
            &base,
            "routing requires at least one route step and one attempt",
        ));
    }

    let mut canonical_route = HashSet::new();
    for (index, step) in inference.routing.route.iter().enumerate() {
        if !canonical_route.insert(step.canonical.as_str()) {
            return Err(producer_error(
                "TRACE_PRODUCER_ROUTE_DUPLICATE",
                format!("{base}/route/{index}/canonical"),
                "authored route repeats a canonical entity",
            ));
        }
    }

    validate_attempts(&inference.routing, &base)?;
    for (index, fill) in inference.routing.non_entity_fill.iter().enumerate() {
        validate_fill(fill, format!("{base}/non_entity_fill/{index}"))?;
    }

    let filled = inference
        .routing
        .attempts
        .iter()
        .enumerate()
        .filter(|(_, attempt)| attempt.outcome == "filled")
        .collect::<Vec<_>>();
    if filled.len() > 1 {
        return Err(producer_error(
            "TRACE_PRODUCER_MULTIPLE_FILLED",
            format!("{base}/attempts"),
            "routing facts contain more than one filled attempt",
        ));
    }

    let requested_entity = inference.routing.route[0].canonical.clone();
    let mut fill = Vec::with_capacity(1 + inference.routing.non_entity_fill.len());
    let mut resolution_refusals = Vec::new();
    let (outcome, bound, entity_fill) = if let Some((attempt_index, attempt)) = filled.first() {
        if inference.routing.refusal.is_some() {
            return Err(producer_error(
                "TRACE_PRODUCER_REFUSAL_CONFLICT",
                format!("{base}/refusal"),
                "routing cannot contain both a filled attempt and a refusal fact",
            ));
        }
        if *attempt_index + 1 != inference.routing.attempts.len() {
            return Err(producer_error(
                "TRACE_PRODUCER_ATTEMPT_ORDER_INVALID",
                format!("{base}/attempts/{attempt_index}"),
                "no routing attempt may follow the filled attempt",
            ));
        }
        let applied = attempt.bound.clone().ok_or_else(|| {
            producer_error(
                "TRACE_PRODUCER_INPUT_INVALID",
                format!("{base}/attempts/{attempt_index}/bound"),
                "filled attempt has no bound entity",
            )
        })?;
        let route_index = inference
            .routing
            .route
            .iter()
            .position(|step| step.canonical == applied);

        let entity = match route_index {
            Some(0) if *attempt_index == 0 => json!({
                "kind": "entity",
                "requested": requested_entity,
                "applied": applied,
                "status": "kept",
                "policy": inference.routing.policy,
            }),
            Some(route_index) if route_index == *attempt_index => {
                validate_fallback_prefix(&inference.routing, route_index, &base)?;
                json!({
                    "kind": "entity",
                    "requested": requested_entity,
                    "applied": applied,
                    "status": "fallback",
                    "policy": inference.routing.policy,
                    "route_step": route_index,
                })
            }
            Some(_) => {
                return Err(producer_error(
                    "TRACE_PRODUCER_ATTEMPT_ROUTE_MISMATCH",
                    format!("{base}/attempts/{attempt_index}"),
                    "filled attempt does not align with its authored route step",
                ));
            }
            None if inference.routing.policy == "best-effort" => json!({
                "kind": "entity",
                "requested": requested_entity,
                "applied": applied,
                "status": "substituted",
                "policy": "best-effort",
                "reason": "ENTITY_SUBSTITUTED",
                "deciding_authority": "venue-operations",
            }),
            None => {
                return Err(producer_error(
                    "TRACE_PRODUCER_SUBSTITUTION_NOT_AUTHORIZED",
                    format!("{base}/attempts/{attempt_index}/bound"),
                    "an applied entity outside the authored route requires best-effort policy",
                ));
            }
        };
        ("invoked", Some(applied), entity)
    } else {
        let refusal = inference.routing.refusal.as_ref().ok_or_else(|| {
            producer_error(
                "TRACE_PRODUCER_REFUSAL_REQUIRED",
                format!("{base}/refusal"),
                "routing with no filled attempt requires a refusal fact",
            )
        })?;
        if !refusal.recourse.is_object() {
            return Err(producer_error(
                "TRACE_PRODUCER_REFUSAL_INVALID",
                format!("{base}/refusal/recourse"),
                "refusal recourse must be a JSON object",
            ));
        }
        let mut resolution_refusal = Map::new();
        resolution_refusal.insert(
            "ref".to_owned(),
            Value::String(
                inference
                    .routing
                    .attempts
                    .last()
                    .expect("routing attempts were validated as non-empty")
                    .reference
                    .clone(),
            ),
        );
        resolution_refusal.insert("reason".to_owned(), Value::String(refusal.reason.clone()));
        resolution_refusal.insert(
            "authority".to_owned(),
            Value::String(refusal.authority.clone()),
        );
        resolution_refusal.insert("recourse".to_owned(), refusal.recourse.clone());
        insert_optional(
            &mut resolution_refusal,
            "triggering_step",
            refusal.triggering_step.as_ref(),
        );
        insert_optional(
            &mut resolution_refusal,
            "policy_decision_id",
            refusal.policy_decision_id.as_ref(),
        );
        insert_optional(
            &mut resolution_refusal,
            "policy_bundle",
            refusal.policy_bundle.as_ref(),
        );
        insert_optional(
            &mut resolution_refusal,
            "policy_engine",
            refusal.policy_engine.as_ref(),
        );
        resolution_refusals.push(Value::Object(resolution_refusal));
        (
            "refused",
            None,
            json!({
                "kind": "entity",
                "requested": requested_entity,
                "status": "refused",
                "policy": inference.routing.policy,
                "reason": refusal.reason,
                "deciding_authority": refusal.authority,
            }),
        )
    };

    fill.push(entity_fill);
    fill.extend(inference.routing.non_entity_fill);
    let mut routing = Map::new();
    routing.insert(
        "requested".to_owned(),
        Value::String(inference.routing.requested),
    );
    routing.insert("policy".to_owned(), Value::String(inference.routing.policy));
    routing.insert("outcome".to_owned(), Value::String(outcome.to_owned()));
    if let Some(bound) = &bound {
        routing.insert("bound".to_owned(), Value::String(bound.clone()));
    }
    routing.insert(
        "attempts".to_owned(),
        serde_json::to_value(&inference.routing.attempts)
            .expect("serializing producer attempts cannot fail"),
    );
    routing.insert("fill".to_owned(), Value::Array(fill));

    let mut bindings = inference
        .routing
        .route
        .iter()
        .map(|step| {
            json!({
                "ref": step.reference,
                "bound": step.canonical,
                "rule": step.rule,
                "ambiguity_surfaced": step.ambiguity_surfaced,
            })
        })
        .collect::<Vec<_>>();
    if let Some(bound) = &bound
        && !inference
            .routing
            .route
            .iter()
            .any(|step| step.canonical == *bound)
    {
        bindings.push(json!({
            "ref": bound,
            "bound": bound,
            "rule": "venue-best-effort-substitution",
            "ambiguity_surfaced": false,
        }));
    }
    let step_id = format!("{}:step:user-content", inference.id);
    let segment_id = format!("{}:segment:0", inference.id);

    Ok(json!({
        "id": inference.id,
        "event": "inference",
        "kind": inference.kind,
        "routing": routing,
        "segments": [{
            "i": 0,
            "id": segment_id,
            "origin": "user-authored",
            "content": { "state": "inline", "text": inference.compiled_request_utf8 },
            "step": step_id,
        }],
        "steps": [{ "id": step_id, "kind": "user-content", "tier": "user" }],
        "resolution": { "bindings": bindings, "refusals": resolution_refusals },
    }))
}

fn validate_attempts(routing: &RoutingInput, base: &str) -> Result<(), TraceProducerError> {
    for (index, attempt) in routing.attempts.iter().enumerate() {
        let pointer = format!("{base}/attempts/{index}");
        let Some(route_step) = routing.route.get(index) else {
            return Err(producer_error(
                "TRACE_PRODUCER_ATTEMPT_ROUTE_MISMATCH",
                &pointer,
                "each route step permits one attempt; retries must be collapsed before input",
            ));
        };
        if attempt.reference != route_step.reference {
            return Err(producer_error(
                "TRACE_PRODUCER_ATTEMPT_ROUTE_MISMATCH",
                format!("{pointer}/ref"),
                "attempt does not reference its authored route step",
            ));
        }
        match attempt.outcome.as_str() {
            "filled" => {
                if attempt.bound.as_deref().is_none_or(str::is_empty) {
                    return Err(producer_error(
                        "TRACE_PRODUCER_ATTEMPT_INVALID",
                        format!("{pointer}/bound"),
                        "filled attempt requires a bound entity",
                    ));
                }
                validate_measurement(attempt.measured.as_ref(), format!("{pointer}/measured"))?;
            }
            "failed" | "blocked" => {
                if attempt.reasons.as_ref().is_none_or(|reasons| {
                    reasons.is_empty() || reasons.iter().any(String::is_empty)
                }) {
                    return Err(producer_error(
                        "TRACE_PRODUCER_ATTEMPT_INVALID",
                        format!("{pointer}/reasons"),
                        "failed or blocked attempt requires at least one reason",
                    ));
                }
            }
            _ => {
                return Err(producer_error(
                    "TRACE_PRODUCER_ATTEMPT_INVALID",
                    format!("{pointer}/outcome"),
                    "attempt outcome must be filled, failed, or blocked",
                ));
            }
        }
    }
    Ok(())
}

fn validate_measurement(
    measured: Option<&Value>,
    pointer: impl Into<String>,
) -> Result<(), TraceProducerError> {
    if measured.is_some_and(Value::is_object) {
        Ok(())
    } else {
        Err(producer_error(
            "TRACE_PRODUCER_ATTEMPT_INVALID",
            pointer,
            "measurement must be a JSON object",
        ))
    }
}

fn validate_fill(fill: &Value, pointer: impl Into<String>) -> Result<(), TraceProducerError> {
    let pointer = pointer.into();
    let Some(fill) = fill.as_object() else {
        return Err(producer_error(
            "TRACE_PRODUCER_FILL_INVALID",
            &pointer,
            "fill entry must be a JSON object",
        ));
    };
    for field in ["kind", "status"] {
        if fill
            .get(field)
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(producer_error(
                "TRACE_PRODUCER_FILL_INVALID",
                format!("{pointer}/{field}"),
                format!("fill entry requires a non-empty {field}"),
            ));
        }
    }
    Ok(())
}

fn validate_fallback_prefix(
    routing: &RoutingInput,
    route_index: usize,
    base: &str,
) -> Result<(), TraceProducerError> {
    for index in 0..route_index {
        let attempt = routing.attempts.get(index).ok_or_else(|| {
            producer_error(
                "TRACE_PRODUCER_ATTEMPT_ROUTE_MISMATCH",
                format!("{base}/attempts"),
                "fallback is missing a preceding route attempt",
            )
        })?;
        if attempt.reference != routing.route[index].reference
            || !matches!(attempt.outcome.as_str(), "failed" | "blocked")
        {
            return Err(producer_error(
                "TRACE_PRODUCER_ATTEMPT_ROUTE_MISMATCH",
                format!("{base}/attempts/{index}"),
                "fallback requires each preceding authored route step to fail or be blocked",
            ));
        }
    }
    if routing.attempts[route_index].reference != routing.route[route_index].reference {
        return Err(producer_error(
            "TRACE_PRODUCER_ATTEMPT_ROUTE_MISMATCH",
            format!("{base}/attempts/{route_index}/ref"),
            "filled fallback attempt does not reference its authored route step",
        ));
    }
    Ok(())
}

fn produce_boundary(
    boundary: BoundaryInput,
    boundary_index: usize,
    inline_threshold_bytes: usize,
) -> Result<Value, TraceProducerError> {
    let base = format!("/boundaries/{boundary_index}");
    if !matches!(
        boundary.outcome.as_str(),
        "succeeded" | "failed" | "blocked"
    ) {
        return Err(producer_error(
            "TRACE_PRODUCER_BOUNDARY_INVALID",
            format!("{base}/outcome"),
            "boundary outcome must be succeeded, failed, or blocked",
        ));
    }
    if matches!(boundary.outcome.as_str(), "failed" | "blocked")
        && boundary.reason.as_deref().is_none_or(str::is_empty)
    {
        return Err(producer_error(
            "TRACE_PRODUCER_BOUNDARY_INVALID",
            format!("{base}/reason"),
            "failed or blocked boundary requires a reason",
        ));
    }
    validate_content(
        &boundary.sent,
        format!("{base}/sent"),
        inline_threshold_bytes,
    )?;
    if let Some(received) = &boundary.received {
        validate_content(received, format!("{base}/received"), inline_threshold_bytes)?;
    }
    for (index, fill) in boundary.fill.iter().enumerate() {
        validate_fill(fill, format!("{base}/fill/{index}"))?;
    }
    if !boundary.measured.is_object() {
        return Err(producer_error(
            "TRACE_PRODUCER_BOUNDARY_INVALID",
            format!("{base}/measured"),
            "measurement must be a JSON object",
        ));
    }

    let mut output = Map::new();
    output.insert("id".to_owned(), Value::String(boundary.id));
    output.insert("event".to_owned(), Value::String("boundary".to_owned()));
    output.insert("tool".to_owned(), Value::String(boundary.tool));
    output.insert("outcome".to_owned(), Value::String(boundary.outcome));
    output.insert("sent".to_owned(), boundary.sent);
    if let Some(received) = boundary.received {
        output.insert("received".to_owned(), received);
    }
    if let Some(reason) = boundary.reason {
        output.insert("reason".to_owned(), Value::String(reason));
    }
    output.insert("fill".to_owned(), Value::Array(boundary.fill));
    if let Some(internal) = boundary.internal {
        output.insert("internal".to_owned(), Value::String(internal));
    }
    output.insert("measured".to_owned(), boundary.measured);
    Ok(Value::Object(output))
}

fn validate_content(
    content: &Value,
    pointer: impl Into<String>,
    inline_threshold_bytes: usize,
) -> Result<(), TraceProducerError> {
    let pointer = pointer.into();
    let Some(content) = content.as_object() else {
        return Err(producer_error(
            "TRACE_PRODUCER_CONTENT_INVALID",
            &pointer,
            "content must be a JSON object",
        ));
    };
    let Some(state) = content.get("state").and_then(Value::as_str) else {
        return Err(producer_error(
            "TRACE_PRODUCER_CONTENT_INVALID",
            format!("{pointer}/state"),
            "content requires a string state",
        ));
    };
    if state != "inline" {
        return Err(producer_error(
            "TRACE_PRODUCER_CONTENT_INVALID",
            format!("{pointer}/state"),
            "the 0.1-draft producer accepts only inline boundary content",
        ));
    }
    let Some(text) = content.get("text").and_then(Value::as_str) else {
        return Err(producer_error(
            "TRACE_PRODUCER_CONTENT_INVALID",
            format!("{pointer}/text"),
            "inline content requires text",
        ));
    };
    if text.len() >= inline_threshold_bytes {
        return Err(producer_error(
            "TRACE_PRODUCER_CONTENT_TOO_LARGE",
            format!("{pointer}/text"),
            "content must be externalized at the declared inline threshold",
        ));
    }
    Ok(())
}

fn insert_optional(output: &mut Map<String, Value>, key: &str, value: Option<&String>) {
    if let Some(value) = value {
        output.insert(key.to_owned(), Value::String(value.clone()));
    }
}

fn producer_error(
    code: &str,
    pointer: impl Into<String>,
    message: impl Into<String>,
) -> TraceProducerError {
    TraceProducerError {
        code: code.to_owned(),
        pointer: pointer.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::produce_trace_json;
    use serde_json::Value;

    #[test]
    fn derives_kept_without_an_expected_trace() {
        let trace = produce_trace_json(include_bytes!("../tests/trace-producer/input-kept.json"))
            .expect("kept trace");
        assert_eq!(
            trace.pointer("/events/0/routing/fill/0/status"),
            Some(&"kept".into())
        );
    }

    #[test]
    fn rejects_strict_substitution() {
        let error = produce_trace_json(include_bytes!(
            "../tests/trace-producer/input-strict-substitution.json"
        ))
        .expect_err("strict substitution must fail");
        assert_eq!(error.code, "TRACE_PRODUCER_SUBSTITUTION_NOT_AUTHORIZED");
    }

    fn kept_input() -> Value {
        serde_json::from_slice(include_bytes!("../tests/trace-producer/input-kept.json"))
            .expect("fixture is valid JSON")
    }

    #[test]
    fn rejects_a_filled_attempt_without_measurements() {
        let mut input = kept_input();
        input["inferences"][0]["routing"]["attempts"][0]["measured"] = Value::Null;
        let error = produce_trace_json(&serde_json::to_vec(&input).unwrap())
            .expect_err("filled attempts require measurements");
        assert_eq!(error.code, "TRACE_PRODUCER_ATTEMPT_INVALID");
        assert_eq!(error.pointer, "/inferences/0/routing/attempts/0/measured");
    }

    #[test]
    fn rejects_inline_content_at_the_externalization_threshold() {
        let mut input = kept_input();
        input["inferences"][0]["compiled_request_utf8"] = Value::String("x".repeat(4096));
        let error = produce_trace_json(&serde_json::to_vec(&input).unwrap())
            .expect_err("content at the threshold cannot remain inline");
        assert_eq!(error.code, "TRACE_PRODUCER_CONTENT_TOO_LARGE");
        assert_eq!(error.pointer, "/inferences/0/compiled_request_utf8");
    }

    #[test]
    fn rejects_a_failed_attempt_without_reasons() {
        let mut input: Value =
            serde_json::from_slice(include_bytes!("../tests/trace-producer/input-refused.json"))
                .unwrap();
        input["inferences"][0]["routing"]["attempts"][0]["reasons"] = Value::Null;
        let error = produce_trace_json(&serde_json::to_vec(&input).unwrap())
            .expect_err("failed attempts require reasons");
        assert_eq!(error.code, "TRACE_PRODUCER_ATTEMPT_INVALID");
        assert_eq!(error.pointer, "/inferences/0/routing/attempts/0/reasons");
    }

    #[test]
    fn rejects_schema_less_boundary_values() {
        let mut input: Value = serde_json::from_slice(include_bytes!(
            "../tests/trace-producer/input-boundary-succeeded.json"
        ))
        .unwrap();
        input["boundaries"][0]["sent"] = Value::Null;
        let error = produce_trace_json(&serde_json::to_vec(&input).unwrap())
            .expect_err("boundary content must be structured");
        assert_eq!(error.code, "TRACE_PRODUCER_CONTENT_INVALID");
        assert_eq!(error.pointer, "/boundaries/0/sent");

        let mut input: Value = serde_json::from_slice(include_bytes!(
            "../tests/trace-producer/input-boundary-succeeded.json"
        ))
        .unwrap();
        input["boundaries"][0]["measured"] = Value::Null;
        let error = produce_trace_json(&serde_json::to_vec(&input).unwrap())
            .expect_err("boundary measurements must be structured");
        assert_eq!(error.code, "TRACE_PRODUCER_BOUNDARY_INVALID");
        assert_eq!(error.pointer, "/boundaries/0/measured");
    }

    #[test]
    fn rejects_retries_as_extra_route_attempts() {
        let mut input = kept_input();
        let first = input["inferences"][0]["routing"]["attempts"][0].clone();
        input["inferences"][0]["routing"]["attempts"] = Value::Array(vec![
            serde_json::json!({
                "ref": "@model:example/atlas-4@2026-08-01!",
                "outcome": "failed",
                "reasons": ["TRANSIENT"]
            }),
            first,
        ]);
        let error = produce_trace_json(&serde_json::to_vec(&input).unwrap())
            .expect_err("producer input has one attempt per route step");
        assert_eq!(error.code, "TRACE_PRODUCER_ATTEMPT_ROUTE_MISMATCH");
        assert_eq!(error.pointer, "/inferences/0/routing/attempts/1");
    }

    #[test]
    fn refusal_names_the_last_attempted_route_step() {
        let mut input: Value = serde_json::from_slice(include_bytes!(
            "../tests/trace-producer/input-fallback.json"
        ))
        .unwrap();
        input["inferences"][0]["routing"]["attempts"][1] = serde_json::json!({
            "ref": "@model:example/atlas-mini@2026-08-01",
            "outcome": "failed",
            "reasons": ["ENTITY_UNAVAILABLE"]
        });
        input["inferences"][0]["routing"]["refusal"] = serde_json::json!({
            "reason": "FALLBACK_EXHAUSTED",
            "authority": "venue-operations",
            "recourse": { "available": true, "kind": "retry" }
        });
        let trace = produce_trace_json(&serde_json::to_vec(&input).unwrap()).unwrap();
        assert_eq!(
            trace.pointer("/events/0/resolution/refusals/0/ref"),
            Some(&Value::String(
                "@model:example/atlas-mini@2026-08-01".to_owned()
            ))
        );
    }
}
