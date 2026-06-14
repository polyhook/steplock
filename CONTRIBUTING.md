# Contributing

## Prerequisites

- [Rust](https://rustup.rs) stable (includes `cargo`, `rustfmt`, `clippy`)
- [`jq`](https://jqlang.github.io/jq/) — required at runtime by `ack.sh`

## Build

```sh
cargo build --manifest-path core/Cargo.toml
```

Release binary:

```sh
cargo build --manifest-path core/Cargo.toml --release
# binary: core/target/release/steplock
```

## Test

```sh
cargo test --manifest-path core/Cargo.toml
```

## Code quality

CI enforces all three — run them before pushing:

```sh
cargo fmt  --manifest-path core/Cargo.toml
cargo clippy --manifest-path core/Cargo.toml --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --manifest-path core/Cargo.toml --no-deps
```

## Project layout

```
core/          Rust crate (library + binary)
core/scripts/  Shell scripts embedded in the binary (ack.sh)
schemas/       JSON Schema files for config.toml and state.json
examples/      Working reference configurations
```

## Making changes

- **New checklist logic** — add to `core/src/run.rs`; unit tests live in the same file
- **CEL evaluation** — `core/src/cel_eval.rs`
- **Mermaid parsing** — `core/src/flow.rs`
- **Shell scripts** — edit `core/scripts/ack.sh`; the binary embeds it via `include_str!` so a rebuild picks up your changes
- **Schema** — update `schemas/checklist-config.schema.json` and the corresponding Rust struct in `core/src/config.rs`

## Submitting a pull request

1. Fork and create a branch off `main`
2. Make your change with tests
3. Ensure `cargo test`, `cargo fmt --check`, and `cargo clippy` all pass
4. Open a draft PR — the CI suite runs automatically
5. Mark as ready when CI is green
