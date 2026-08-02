# PromptSyntax-rs

A conservative, provenance-aware parser for the Prompt Syntax control plane.

The parser separates authored control-plane islands from ordinary data-plane text. It
does not resolve models, tools, files, or skills itself. A host supplies the bare entity
and action names available in its capability envelope; qualified references parse
without that lookup. Escaped, fenced, email-like, and inert input stays text.

```rust
use promptsyntax::{Parser, Segment};

let parsed = Parser::new()
    .entity("opus")
    .action("concise")
    .parse("@opus Summarize @file:q3.md /concise");

assert_eq!(parsed.data_plane(), " Summarize  ");
assert_eq!(parsed.directives().count(), 3);
assert!(matches!(parsed.segments[0], Segment::Directive(_)));
```

The crate is available from crates.io as [`promptsyntax`](https://crates.io/crates/promptsyntax)
under the MIT license.

## Scope

- point references and actions, including full-width sigils
- strict markers and JSON5-style scalar argument lists
- `limit(...)` fallback routes
- `<ps ...>...</ps>` spans, with recursively parsed authored inner text
- strict `---ps` frontmatter envelopes
- source byte ranges, typed diagnostics, and explicit data/control-plane projections
- bidi-control rejection and fail-closed handling of malformed qualified islands

Resolution, authorization, canonical pinning, execution, and Prompt Trace generation are
separate layers and intentionally remain host responsibilities.

See [`docs/agencyzero.md`](docs/agencyzero.md) for the extraction path from AgencyZero's
current reverse-channel parser.

## Development

```bash
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
```

## Core conformance adapter

`ps-core-adapter` evaluates the specification-owned Core corpus and emits the normalized
language-neutral result consumed by the PromptSyntax differential runner. The adapter owns
no expected answers. Supply the canonical corpus path and the exact commit under test:

```bash
cargo run --bin ps-core-adapter -- \
  /path/to/promptsyntax.org/conformance/cases/core-parser.json \
  0123456789abcdef0123456789abcdef01234567
```

The normalized result uses UTF-8 byte offsets and includes the complete segment tree,
directive AST, source slices, and parser diagnostics.

Pass `--jsonl` after the commit to stream one compact normalized result per line. The first
line contains implementation metadata. This mode is intended for large generated
differential corpora and does not retain all result objects in memory.

## Trace producer

`ps-trace-producer` derives a user-tier Prompt Trace from deterministic execution facts. It
does not receive an expected trace or the independent transcript used by the conformance
runner:

```bash
cargo run --bin ps-trace-producer -- producer-input.json
```

The producer derives kept, authored fallback, best-effort substitution, and refusal states.
Contradictory facts, including strict substitution or multiple filled attempts, produce a
typed JSON error and exit status `1`. The current contract is deliberately limited to the
`0.1-draft` user-tier executed profile.
