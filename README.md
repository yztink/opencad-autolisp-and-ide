# OpenCAD AutoLISP Runner & OCAIDE Editor Plugin

> [!WARNING]
> **Status: Extreme Alpha / Active Development**  
> This project is in an early experimental stage and under active development. APIs, internal structures, and features are subject to rapid change. Expect rough edges and ongoing improvements.

A complete AutoLISP scripting, interactive command runner, and integrated development environment (OCAIDE) for **OpenCAD Studio**.

Built with Rust and [Iced](https://github.com/iced-rs/iced), this plugin enables loading, running, and debugging AutoLISP routines and scripts directly inside OpenCAD Studio with zero custom modifications required to the CAD engine.

---

## Features

- **Integrated OCAIDE Editor**: A full-featured native GUI development environment tailored for AutoLISP:
  - Tabbed editor buffers with syntax highlighting and line numbers.
  - S-expression bracket matching, structural auto-indent, and smart Enter.
  - Visual LISP (VLIDE) formatting conventions and symbol casing normalization.
  - In-editor Find & Replace bar (`Ctrl+F`).
  - Interactive REPL console with command history navigation.
- **Visual S-Expression Debugger**:
  - Breakpoint toggling on expressions or lines (`F9`).
  - Expression stepping: Step Over (`F10`), Step Into (`F11`), and Step Out (`Shift+F11`).
  - Live call stack frame inspection.
  - Real-time watch window (`Shift+F9`) and variable inspection.
  - Non-blocking keepalive heartbeat that maintains host connection during paused debugging.
- **Top-Level CAD Command Registration**:
  - Functions defined with `(defun c:NAME () ...)` are immediately registered and executable from the CAD command bar without any special prefix.
- **CAD Canvas Integration**:
  - Interactive point picking (`(getpoint)` / `(getdist)` / `(getangle)`) with real-time rubber-band feedback.
  - Entity creation (`(entmake)`), inspection (`(entget)`), and updating (`(entmod)`).
  - Selection set handling (`(ssget)`, `(sslength)`, `(ssname)`).
  - Symbol table access (`(tblsearch)` and `(tblnext)` for layers, line types, text styles, blocks, views, etc.).
- **Dynamic GUI Panels & Dialogs**:
  - Custom sidebar panels via `(ocl-panel-register ...)` and `(ocl-panel-get ...)`.
  - DCL and native dialog prompts (`(ocl-dialog ...)`).
- **Dual Mode Execution**:
  - Run embedded inside OpenCAD Studio via the ribbon button or typing `OCAIDE`.
  - Run standalone (`cargo run --bin opencad_autolisp_ide`) to edit and inspect AutoLISP routines independently.

---

## Architecture

This plugin connects OpenCAD Studio to the authoritative **[opencad-autolisp-engine](../opencad-autolisp-engine)**:
- **`src/rust_lisp.rs`**: Implements the `CadBridge` trait against OpenCAD Studio's V4 host IPC.
- **`src/ide/`**: Contains the full Iced-based OCAIDE interface, LSP/RPC server, and debugging coordinator.
- **`src/lib.rs`**: Exposes the OpenCAD Studio V4 C-ABI plugin entry points (`on_load`, `dispatch`, `on_notification`).

---

## Building & Installation

### Prerequisites
- Rust 1.80+ (edition 2021)
- OpenCAD Studio (v4 plugin API compatible)

### Build (Release)
```bash
cargo build --release
```

This compiles:
- `target/release/opencad_autolisp.dll` (or `.so` / `.dylib`): The OpenCAD Studio plugin library.
- `target/release/opencad_autolisp_ide.exe`: The standalone OCAIDE IDE application.

### Installation in OpenCAD Studio
Copy `plugin.toml` and the compiled library into your OpenCAD Studio user plugins folder:
- **Windows**:
  - Copy `plugin.toml` &rarr; `%APPDATA%\OpenCADStudio\plugins\opencad.autolisp_and_ide\plugin.toml`
  - Copy `target\release\opencad_autolisp.dll` &rarr; `%APPDATA%\OpenCADStudio\plugins\opencad.autolisp_and_ide\opencad.autolisp_and_ide.dll`
- **Linux**:
  - Copy `plugin.toml` &rarr; `~/.config/OpenCADStudio/plugins/opencad.autolisp_and_ide/plugin.toml`
  - Copy `target/release/libopencad_autolisp.so` &rarr; `~/.config/OpenCADStudio/plugins/opencad.autolisp_and_ide/opencad.autolisp_and_ide.so`

---

## Quick Start in AutoLISP

Save your `.lsp` routines in your scripts directory (e.g. `%APPDATA%\OpenCADStudio\scripts\` on Windows):

```lisp
;; Define a custom command: DRAW_BOX
(defun c:draw_box (/ pt1 pt2)
  (princ "\n=== Draw Box Routine ===")
  (setq pt1 (getpoint "\nSpecify first corner: "))
  (setq pt2 (getpoint pt1 "\nSpecify opposite corner: "))
  (if (and pt1 pt2)
    (progn
      ;; Draw 4 border lines
      (command "_.LINE" pt1 (list (car pt2) (cadr pt1) 0.0) "")
      (command "_.LINE" (list (car pt2) (cadr pt1) 0.0) pt2 "")
      (command "_.LINE" pt2 (list (car pt1) (cadr pt2) 0.0) "")
      (command "_.LINE" (list (car pt1) (cadr pt2) 0.0) pt1 "")
      (princ "\nBox created successfully!")
    )
  )
  (princ)
)
```

Type `DRAW_BOX` in the OpenCAD Studio console to run it, or open `OCAIDE` to set breakpoints and step through execution!

---

## Testing

Run the full automated test suite (90 unit and integration tests):
```bash
cargo test --lib
```

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for pull request guidelines, repository boundaries, CAD conventions, and testing expectations. AI coding tools should also follow [AGENTS.md](AGENTS.md).

---

## License

GNU General Public License v3.0 (GPL-3.0-only).
