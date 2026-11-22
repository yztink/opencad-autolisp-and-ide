# Contributing to OpenCAD AutoLISP & IDE

Thank you for contributing! This guide summarizes our engineering standards for pull requests in `opencad-autolisp-and-ide` (see [AGENTS.md](./AGENTS.md) for the full technical reference).

## Repository Boundaries

| Layer | Repository / Path |
|---|---|
| AutoLISP core language & stdlib | `opencad-autolisp-engine` (`crates/lisp-core`) |
| CAD bridge traits & standard helpers | `opencad-autolisp-engine` (`crates/lisp-cad`) |
| OpenCAD Studio plugin wiring & document context | `src/rust_lisp.rs`, `src/interactive.rs`, `src/context.rs` |
| AutoLISP IDE UI | `src/ide/` |
| Host CAD engine, plugin API, canvas & command line | `OpenCADStudio` |

> **Note:** `archive/python_shelved/` is frozen for archival purposes only. All AutoLISP execution runs through the Rust engine (`opencad-autolisp-engine`); do not add features or bug fixes to the Python archive.

## Core Principles

1. **Fix the right layer — avoid workarounds.**
   If a bug or missing capability originates in `OpenCADStudio` (host plugin API, command/interaction routing, rendering, document state) or `opencad-autolisp-engine` (evaluator, standard library, bridge traits), fix it in that repository (and link the paired PR) instead of adding timing hacks, synthetic workarounds, or duplicated logic in the plugin.
2. **Adhere to established CAD standards.**
   Preserve standard AutoLISP semantics, DXF/DWG object and group-code conventions, coordinate/unit behavior, and clean CAD command-line interaction.
3. **Keep naming clean.**
   Do not mention names of third-party or proprietary CAD software products, commercial add-on libraries, or CAD vendors in code, comments, documentation, commit messages, or PR descriptions.
4. **Keep changes focused and portable.**
   Keep diffs scoped to your change (avoid unrelated reformatting or bulk `rustfmt`), handle errors safely across script and host boundaries, and ensure `Cargo.toml` does not retain local-only path overrides.

## Testing & Pull Requests

1. Run the plugin test suite locally before submitting:
   ```bash
   cargo test --features ide-gui
   ```
2. Open your PR using [.github/PULL_REQUEST_TEMPLATE.md](./.github/PULL_REQUEST_TEMPLATE.md), including **Problem & Why This PR Is Needed**, a clear summary of changes, any paired `OpenCADStudio` or `opencad-autolisp-engine` PR dependencies, and what you tested.
