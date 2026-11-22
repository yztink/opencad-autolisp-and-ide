# AGENTS.md

Repository instructions for AI coding agents working in `opencad-autolisp-and-ide`.

## Architecture & Repository Boundaries

This repository hosts the OpenCAD Studio AutoLISP plugin and integrated development environment. Work cleanly across the layer boundaries:

| Concern | Authoritative Location |
|---|---|
| Core AutoLISP lexer, parser, evaluator, environment, standard library | `opencad-autolisp-engine/crates/lisp-core` |
| Host-agnostic CAD bridge traits and CAD standard helpers | `opencad-autolisp-engine/crates/lisp-cad` |
| OpenCAD Studio plugin host wiring, command dispatch, CAD document/context bindings | `src/rust_lisp.rs`, `src/interactive.rs`, `src/context.rs` |
| Integrated AutoLISP editor / debugger UI | `src/ide/` |
| Host CAD canvas, command line, host plugin API (`ocs_plugin_api`), document & rendering engine | `OpenCADStudio` (upstream host repository) |

- **Python runtime is shelved**: `archive/python_shelved/` is frozen behind the off-by-default `python` feature. All AutoLISP evaluation routes exclusively through `opencad-autolisp-engine` via `src/rust_lisp.rs`. Never mirror Rust AutoLISP changes into Python.

## Fix the Right Layer — No Workarounds

- Diagnose root causes and implement fixes in the authoritative repository (`OpenCADStudio`, `opencad-autolisp-engine`, or `opencad-autolisp-and-ide`) rather than introducing plugin-side workarounds when modifying OpenCAD Studio or the AutoLISP engine is the proper engineering solution.
- Do **not** paper over missing host plugin API capabilities, host command/interaction routing issues, or engine evaluator/trait defects using timing sleeps, polling loops, duplicate parallel implementations, synthetic command-history injection, or fragile output scraping.
- When a proper fix requires changes in `OpenCADStudio` or `opencad-autolisp-engine`:
  1. Make or coordinate the change in that upstream repository first (or as a paired PR).
  2. Call out the cross-repository dependency and ordering explicitly in your PR description.

## CAD Standards & Language Fidelity

- Adhere strictly to established CAD standards, DXF/DWG object models and group-code conventions, and standard AutoLISP language semantics.
- Maintain faithful coordinate, unit, selection, and interactive workflow behavior matching standard CAD command-line expectations.
- Keep user-facing command output and command history clean; never expose internal bridge markers or synthetic helper tokens in normal CAD output.
- Update reference documentation when changing or adding user-facing language behavior or built-in functions.

## Naming & Trademark Hygiene

- **Never** mention the names of third-party or proprietary CAD software products, commercial add-on libraries, or CAD vendors anywhere in code, comments, identifiers, docstrings, tests, documentation, commit messages, or PR descriptions.
- Refer exclusively to OpenCAD Studio, AutoLISP, DXF/DWG, and open CAD standards.

## Engineering & PR Best Practices

- **Tightly scoped diffs**: Keep changes focused on the task; preserve existing unrelated comments, formatting, and code. Never run bulk formatting (`rustfmt`) across untouched files.
- **Error handling**: Propagate structured errors (`Result` / domain error types) rather than panicking (`unwrap()` / `expect()`) on user script input, document state, or host boundary calls.
- **ABI & dependency portability**: Respect host/plugin API compatibility guarantees and keep committed `Cargo.toml` dependencies portable (never commit temporary local path overrides in place of git/versioned dependencies).
- **Verification**: Run the full plugin test suite before finishing:
  ```bash
  cargo test --features ide-gui
  ```
- **PR quality**: Complete all sections of `.github/PULL_REQUEST_TEMPLATE.md`, clearly explaining the problem, why the change is needed, which repository layer owns the fix, any cross-repo PR links, and exact test verification run.
