# Compiling & Plugin Compatibility Guide

This guide explains how to compile the **OpenCAD LISP Runner** plugin (`opencad-lisp-and-ide`) so that it is always 100% compatible with your current version of **OpenCAD Studio**.

---

## 1. Understanding the `acadrust` Compatibility Error

If you see an error like this when starting OpenCAD Studio:

```
Load failed: Plugin built for acadrust @7ea4247134af60c8c1617426849d4fa8233660c8, but this host uses @5b682ed66ea2c89be8142c8dd83d83774fc3de08
```

### Why does this happen?
OpenCAD Studio uses `acadrust` (the CAD codec engine) across its core and plugin API. Because Rust does not have a stable ABI and data types are passed between the host and plugins, OpenCAD Studio strictly validates that any loaded plugin was built against the exact same git commit of `acadrust` as the host.

When OpenCAD Studio updates its `acadrust` dependency (e.g., when merging an upstream bump or chore PR), the plugin must be recompiled with the matching commit, and its `plugin.toml` manifest must reflect that commit.

---

## 2. Automated Sync & Build

We provide a PowerShell script that automatically inspects OpenCAD Studio, aligns dependencies, updates locks, compiles the release binaries, and installs them directly into your `%APPDATA%` plugins directory:

```powershell
# Run from the opencad-lisp-and-ide repository root:
powershell -ExecutionPolicy Bypass -File .\scripts\sync-acadrust.ps1 -Install
```

### What the script does:
1. Auto-locates your `OpenCADStudio-fork` or `OpenCADStudio` repository.
2. Extracts the exact `acadrust` git revision and full 40-character commit hash from `OpenCADStudio/Cargo.lock`.
3. Updates `plugin.toml` with `acadrust_source = "git+https://...#<full_commit_hash>"`.
4. Runs `cargo update -p acadrust` in `opencad-lisp-and-ide`.
5. Compiles release binaries (`cargo build --release`).
6. Deploys `plugin.toml`, `opencad.lisp_and_ide.dll`, and `opencad_lisp_ide.exe` to `%APPDATA%\OpenCADStudio\plugins\opencad.lisp_and_ide\`.

---

## 3. Manual Step-by-Step Procedure

If you prefer to perform each step manually:

### Step 1: Find OpenCAD Studio's `acadrust` Commit
Open `OpenCADStudio-fork/Cargo.lock` (or `OpenCADStudio/Cargo.lock`) and locate `[[package]]` named `acadrust`:
```toml
[[package]]
name = "acadrust"
version = "0.5.5"
source = "git+https://github.com/HakanSeven12/cadcodec.git?rev=5b682ed#5b682ed66ea2c89be8142c8dd83d83774fc3de08"
```
Note the full source string including `#<full_40_char_hash>`.

### Step 2: Update `plugin.toml`
In `plugin.toml`, update `acadrust_source` under `[opencad]` to the exact source string from Step 1:
```toml
[opencad]
api_version = 13
rustc_version = "rustc 1.98.1 (48a229cea 2026-09-01)"
acadrust_source = "git+https://github.com/HakanSeven12/cadcodec.git?rev=5b682ed#5b682ed66ea2c89be8142c8dd83d83774fc3de08"
```
> **Important**: The string **must** end with `#<40-character-hash>`. If the `#hash` suffix is omitted, the host cannot parse the commit and will mark the plugin incompatible.

### Step 3: Update Cargo Lock
In the repository root:
```bash
cargo update -p acadrust
```

### Step 4: Compile Release
```bash
cargo build --release
```

### Step 5: Deploy to OpenCAD Studio Plugins Directory
Copy **both** `plugin.toml` and the compiled library into your user plugins folder:

- **Windows**:
  - Copy `plugin.toml` &rarr; `%APPDATA%\OpenCADStudio\plugins\opencad.lisp_and_ide\plugin.toml`
  - Copy `target\release\opencad_lisp.dll` &rarr; `%APPDATA%\OpenCADStudio\plugins\opencad.lisp_and_ide\opencad.lisp_and_ide.dll`
  - (Optional) Copy `target\release\opencad_lisp_ide.exe` &rarr; `%APPDATA%\OpenCADStudio\plugins\opencad.lisp_and_ide\opencad_lisp_ide.exe`

- **Linux**:
  - Copy `plugin.toml` &rarr; `~/.config/OpenCADStudio/plugins/opencad.lisp_and_ide/plugin.toml`
  - Copy `target/release/libopencad_lisp.so` &rarr; `~/.config/OpenCADStudio/plugins/opencad.lisp_and_ide/opencad.lisp_and_ide.so`

---

## 4. Troubleshooting Checklist

| Issue | Likely Cause | Solution |
| :--- | :--- | :--- |
| `Load failed: Plugin built for acadrust @..., but this host uses @...` | Installed `plugin.toml` or DLL was not updated in `%APPDATA%`. | Re-run `powershell .\scripts\sync-acadrust.ps1 -Install` and ensure OpenCAD Studio is restarted. |
| `[plugin] ... declares acadrust metadata but has no fingerprint` | `plugin.toml` has `acadrust_source` missing the `#<40-char-hash>` suffix. | Ensure `acadrust_source` includes `#<full_hash>`. |
| `Plugin built with rustc ..., host requires ...` | The plugin was built with a different Rust compiler toolchain than OpenCAD Studio. | Update your toolchain (`rustup update`) so both match. |
