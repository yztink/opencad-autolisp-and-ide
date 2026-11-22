## Problem & Why This PR Is Needed

<!-- Describe the problem, broken behavior, or missing capability and why this change is required. Link related issues if applicable. -->

## Summary of Changes

<!-- Concisely explain what changed, which module/layer was updated, and why this design was chosen. -->

## Cross-Repo Dependencies

<!-- Does this PR require paired changes in OpenCADStudio or opencad-autolisp-engine? Link upstream PRs/commits or state "None". -->
- None

## Verification

<!-- List the exact commands and manual checks run (e.g., `cargo test --features ide-gui`). -->
- [ ] `cargo test --features ide-gui` passes

## Checklist

- [ ] Root cause is fixed at the proper layer (`OpenCADStudio`, `opencad-autolisp-engine`, or plugin) without introducing unnecessary workarounds
- [ ] Changes adhere to established CAD standards, DXF/DWG conventions, and standard AutoLISP semantics
- [ ] No names of third-party or proprietary CAD software, add-on libraries, or CAD vendors appear in code, comments, docs, commits, or this PR description
- [ ] `archive/python_shelved/` was left untouched and `Cargo.toml` contains no temporary local-only path overrides
- [ ] Diff is tightly scoped with no unrelated reformatting
