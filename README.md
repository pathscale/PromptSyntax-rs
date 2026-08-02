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

Provider text streams must be decoded before they are rendered. A provider may
split the final `>` of a standalone authoring segment into a later chunk, so
parsing each delta independently can briefly expose control syntax as text.

```rust
use promptsyntax::Parser;

let mut stream = Parser::new()
    .authoring_namespace("agency")
    .authoring_stream();

let first = stream.push(r#"Working.
<ps @agency:items.add(title: "Fix")"#); // held until the line is classifiable
let second = stream.push(">\nContinuing.");
let last = stream.finish();

assert_eq!(first.data_plane, "Working.\n");
assert_eq!(second.data_plane, "Continuing.");
assert_eq!(second.directives.len(), 1);
assert!(last.data_plane.is_empty());
```

`AuthoringStream` preserves ordinary streaming, absolute source byte spans, and
Markdown provenance. Blockquoted, indented, fenced, and inline examples remain
data; only a complete standalone segment in a declared namespace enters the
control plane. A malformed declared segment fails closed with a diagnostic.

The crate is available from crates.io as [`promptsyntax`](https://crates.io/crates/promptsyntax)
under the MIT license.

## Scope

- point references and actions, including full-width sigils
- strict markers and JSON5-style scalar argument lists
- `limit(...)` fallback routes
- `<ps ...>...</ps>` spans, with recursively parsed authored inner text
- strict `---ps` frontmatter envelopes
- source byte ranges, typed diagnostics, and explicit data/control-plane projections
- chunk-boundary-independent decoding for host-declared authoring surfaces
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
