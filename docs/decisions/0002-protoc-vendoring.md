# ADR 0002: protoc vendoring via protoc-bin-vendored

## Status
Accepted

## Context
`tonic-build` needs a `protoc` binary to compile `plugin.proto`. This environment
has no system `protoc` installed, and no `cmake` either.

Two vendoring crates were evaluated:
- `protobuf-src` — builds `protoc` from source via `cmake`. Failed in this
  environment: `cmake` is not installed, so the build script errors out during
  `cargo build` with a CMake-detection failure.
- `protoc-bin-vendored` — ships prebuilt `protoc` binaries for common host
  triples and just resolves a path to one; no compiler toolchain required.

## Decision
Use `protoc-bin-vendored = "3"` as a build-dependency. In `build.rs`:

```rust
std::env::set_var(
    "PROTOC",
    protoc_bin_vendored::protoc_bin_path().expect("failed to locate vendored protoc"),
);
```

then run `tonic_build::configure().compile_protos(...)` as normal. This compiled
cleanly and reproducibly with no system dependencies.

## Consequences
- No system `protoc` or `cmake` required to build this crate anywhere `protoc-bin-vendored`
  ships a binary for the host triple.
- If `protoc-bin-vendored` ever lacks a binary for a target platform we ship on, fall
  back to `protobuf-src` (with a `cmake` build-dependency) for that platform only.
