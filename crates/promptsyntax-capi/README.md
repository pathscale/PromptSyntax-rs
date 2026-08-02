# Prompt Syntax C API

This crate exposes the safe Rust parser through a small, versioned C ABI. It does not
contain another parser implementation.

The interface uses opaque Rust-owned handles and explicit UTF-8 byte spans:

- `ps_parser_t` owns parser configuration.
- `ps_parse_result_t` owns one serialized result.
- `ps_parse_result_json` borrows non-null-terminated JSON bytes from that result.
- each handle must be freed exactly once by the matching Prompt Syntax function.
- every status-returning entry point catches Rust panics before they can unwind into C.

The result schema is `org.promptsyntax.parse-result/0.1`. ABI version 1.0 is encoded as
`0x00010000`; incompatible ABI changes require a new major version.

## Build

```sh
cargo build -p promptsyntax-capi
```

This produces dynamic and static libraries named `promptsyntax_capi`. Include
[`include/promptsyntax.h`](include/promptsyntax.h) from C or C++.

On macOS, the C smoke test can be compiled and run after a debug build with:

```sh
cc -std=c11 -Wall -Wextra -Werror \
  -I crates/promptsyntax-capi/include \
  crates/promptsyntax-capi/tests/smoke.c \
  -L target/debug -lpromptsyntax_capi \
  -Wl,-rpath,@loader_path/debug \
  -o target/capi-smoke
./target/capi-smoke
```

Linux uses the same command with `-Wl,-rpath,'$ORIGIN/debug'`. Windows consumers
link against the generated import library and load `promptsyntax_capi.dll`.

The repository CI builds the Rust workspace on Linux, macOS, and Windows. It also compiles
and executes linked C clients on Linux and macOS and a linked C++17 client on macOS.

## Safety boundary

The core `promptsyntax` crate continues to forbid unsafe Rust. All pointer conversion is
isolated here and documented per function. The ABI accepts invalid UTF-8 and null-pointer
errors as typed statuses; misuse such as double-free, an invalid non-null pointer, or
concurrent mutation remains a caller violation, as in conventional C APIs.
