# Translating OpenCAD LISP & IDE

OpenCAD LISP & IDE uses the same **[Mozilla Project Fluent](https://projectfluent.org/)** (`.ftl`) translation system as OpenCAD Studio. Translation catalogs live under `locales/`:

```text
locales/en-US/opencad_lisp.ftl   Source catalog (English)
locales/es-ES/opencad_lisp.ftl   Spanish
locales/<tag>/opencad_lisp.ftl   Additional languages (BCP 47 tags such as de-DE, fr-FR, tr-TR, ja-JP)
```

## How Language Selection Works

1. **Inside OpenCAD Studio**: OpenCAD Studio passes its active UI language via `OCS_LANGUAGE` (`ocs_plugin_api::host_language()`), which the plugin and OCIDE pick up automatically.
2. **Override inside OCIDE**: Open **Settings › General & Debugger › Interface Language** to choose an explicit language (updates OCIDE live) or keep *Follow OpenCAD Studio / System*.
3. **Fallback**: Any missing or untranslated key falls back to `en-US` automatically.

## Testing a Translation Without Installing Rust

You can drop a translated `.ftl` file directly into your installed plugin's `locales/<tag>/` folder:

```text
%APPDATA%\OpenCADStudio\plugins\opencad.lisp_and_ide\locales\<tag>\opencad_lisp.ftl
```

(Or set `OCS_LOCALE_DIR` to a custom directory containing `<tag>/opencad_lisp.ftl`.) External `.ftl` files override embedded catalogs and appear automatically in OCIDE's **Interface Language** dropdown.

## Editing or Adding a Language

1. Copy `locales/en-US/opencad_lisp.ftl` to `locales/<tag>/opencad_lisp.ftl` (e.g., `locales/de-DE/opencad_lisp.ftl`).
2. Translate the text on the right-hand side of `=`.
3. **Do not translate**:
   - Message keys on the left-hand side of `=`.
   - Placeholder names inside `{ $name }` (you may reorder them within the sentence).
   - CAD command names (`OCIDE`, `APPLOAD`, `STARTUP`, `LSPLIST`, `LSPRELOAD`, `LSPDIR`, `LSPERR`, `LSPRUN`, `LSP`) and LISP symbols (`defun`, `setq`, `princ`, `command`, `T`, `nil`).
   - Plural selectors require a `*` default branch (e.g., `*[other]`).

## Validation Check

If you have Rust installed, run the catalog test to verify Fluent syntax, key coverage, and placeholder parity against `en-US`:

```bash
cargo test --lib i18n::tests
```
