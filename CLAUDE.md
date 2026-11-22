# CLAUDE.md

@AGENTS.md

Quick rules for `opencad-autolisp-and-ide`:

- Fix root causes in the proper layer (`OpenCADStudio`, `opencad-autolisp-engine`, or this plugin) — never build plugin workarounds when modifying OpenCAD Studio or the AutoLISP engine is the right fix.
- Follow established CAD standards, DXF/DWG conventions, and standard AutoLISP semantics.
- Never include names of third-party or proprietary CAD software, add-on libraries, or vendors in code, docs, commits, or PR descriptions.
- Keep `archive/python_shelved/` frozen; all AutoLISP evaluation uses `opencad-autolisp-engine` via `src/rust_lisp.rs`.
- Keep diffs tightly scoped (no bulk `rustfmt`), keep `Cargo.toml` dependencies portable, and run `cargo test --features ide-gui` before opening a PR using `.github/PULL_REQUEST_TEMPLATE.md`.
