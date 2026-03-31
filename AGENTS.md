# Repository Guidelines

## Project Structure & Module Organization
- `src/engine/` contains the primary runtime built on `boa_engine`, including environment setup, value handling, and script/module execution.
- `src/main.rs` is the CLI entry point for `--eval`, file execution, `--module`, and `--test262`.
- `src/lexer/`, `src/parser/`, and `src/engine/interpreter.rs` preserve the handwritten experimental frontend/runtime pieces.
- `tests/` holds integration coverage: `parser_test.rs`, `interpreter_test.rs`, `isolated_test.rs`, and `test262_runner.rs`.
- `run_test262.sh` manages a sparse `test262` checkout and runs the ignored core-profile conformance suite. Helper scripts live in `tmp_scripts/`.

## Build, Test, and Development Commands
- `cargo build` — compile the project.
- `cargo run -- --eval "1 + 2"` — execute inline JavaScript.
- `cargo run -- --module path/to/file.js` — run an ECMAScript module.
- `cargo test` — run the standard Rust test suite.
- `cargo test --test isolated_test` — run engine smoke tests similar to CI.
- `./run_test262.sh` — fetch `test262` if needed and run the ignored core-profile suite.
- `TEST262_FILTER=import-defer TEST262_MAX_CASES=50 cargo test --test test262_runner test262_core_profile -- --ignored --nocapture` — example focused conformance run.

## Coding Style & Naming Conventions
- Use Rust 2024 idioms and `rustfmt` defaults; this repo has no custom formatter config.
- Follow standard Rust naming: `snake_case` for functions/modules/tests, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for constants.
- Keep reusable runtime logic in `src/engine/`; keep CLI-only argument handling in `src/main.rs`.
- Prefer `Result`-based error propagation over new `panic!` paths in engine code.

## Testing Guidelines
- Add or update integration tests in `tests/` for every parser, interpreter, or runtime behavior change.
- Match existing behavior-first test names such as `parser_parses_*`, `interpreter_*`, and `engine_*`.
- For `test262` work, run a narrow filtered case first, then the broader `./run_test262.sh` flow before opening a PR.

## Commit & Pull Request Guidelines
- Recent commits use short, imperative subjects like `Add optional chaining support for member access and calls.` Keep the first line focused and scoped to one change.
- PRs should summarize the affected subsystem, list commands run, and call out any `test262` skips, filters, or behavior changes.
- Include log snippets or reproduction steps when touching CLI output, module loading, or conformance behavior.
