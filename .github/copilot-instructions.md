# Copilot Instructions

Follow [AGENTS.md](../AGENTS.md) when working in `opencad-lisp-and-ide`:

- Fix root causes at the proper layer (`OpenCADStudio`, `opencad-lisp-engine`, or this plugin) rather than creating workarounds when OpenCAD Studio or the OpenCAD LISP engine should be updated instead.
- Adhere to established CAD standards, DXF/DWG specifications, and standard OpenCAD LISP semantics.
- Never include names of third-party or proprietary CAD software products, add-on libraries, or CAD vendors in code, comments, docs, commits, or PR descriptions.
- Keep `archive/python_shelved/` untouched; all OpenCAD LISP execution runs via `opencad-lisp-engine`.
- Keep diffs tightly scoped (no bulk `rustfmt`), preserve portable `Cargo.toml` dependencies, run `cargo test --features ide-gui`, and follow `.github/PULL_REQUEST_TEMPLATE.md`.
