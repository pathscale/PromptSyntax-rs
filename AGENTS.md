# Working agreement

- Use stable Rust and keep `unsafe` forbidden.
- The parser is provenance-aware. Never turn inert, escaped, quoted, or fenced content
  into control-plane syntax.
- Bare references and actions are environment-relative and parse only when the caller
  declares them available. Qualified references may parse without a resolver.
- Preserve byte ranges and source text exactly. Parsing must not silently rewrite user
  input.
- Keep the Rust and TypeScript conformance cases behavior-compatible.
- Run formatting, tests, and strict Clippy before reporting completion.
- Do not publish or push without explicit approval.

