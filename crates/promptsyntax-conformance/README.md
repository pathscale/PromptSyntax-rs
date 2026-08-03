# promptsyntax-conformance

Stable-Rust tooling for checking the language-independent PromptSyntax conformance corpus.

The specification and its versioned vectors are the oracle. This crate checks their shape,
references, identifiers, blockers, and coverage metadata. It does not embed expected parser
or Trace results.

Current command:

```bash
cargo run -p promptsyntax-conformance --bin ps-conformance -- \
  check-requirements /path/to/requirements.json

cargo run -p promptsyntax-conformance --bin ps-conformance -- \
  check-schema /path/to/schema.json

cargo run -p promptsyntax-conformance --bin ps-conformance -- \
  validate-instance /path/to/schema.json /path/to/instance.json
```

The command emits a deterministic JSON report and exits with:

- `0` when the input passes the implemented corpus checks;
- `1` when the input is well-formed enough to report conformance diagnostics; or
- `2` for invalid command usage or an internal report-serialization error.

This is bootstrap tooling for a working draft, not a certification utility.
