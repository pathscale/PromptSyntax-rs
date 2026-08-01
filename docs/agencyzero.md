# AgencyZero integration

Use the Rust parser as the syntax/provenance layer and keep AgencyZero's store mutations
as the resolver/execution layer:

```rust
let parsed = promptsyntax::Parser::new()
    .authoring_namespace("agency")
    .parse(reply);
```

`Directive::AuthoringSegment` carries the qualified reference and decoded arguments from
a standalone `<ps @agency:...>` line. Map that AST into AgencyZero's existing closed
`items.state`, `items.add`, `items.retire`, and `pr.link` enum, then keep the existing
capability-bound checks and typed receipts.

The parser does not authorize the namespace or its verbs. The application declaration
does. An unknown verb must therefore become AgencyZero's `ENTITY_NOT_FOUND` receipt, not
a new capability. A malformed declared segment is returned as
`InvalidAuthoringSegment` with a diagnostic and remains outside `data_plane()`.

Normal forward-channel spans and strict frontmatter share the same parser without
weakening the reverse channel's standalone-line framing rule.

