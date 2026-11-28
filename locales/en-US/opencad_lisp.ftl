### OpenCAD LISP & IDE — English (en-US) Source Catalog
### Keep CAD command names (OCIDE, APPLOAD, LSPLIST, etc.), LISP symbols,
### and { $variable } placeholders untranslated.

## Language Selector
language-auto = Follow OpenCAD Studio / System

## Ribbon
ribbon-tab-title = OpenCAD LISP
ribbon-group-scripting = OpenCAD LISP Scripting
ribbon-tool-ide = OpenCAD LISP IDE
ribbon-tool-appload = Appload
ribbon-tool-startup = Startup
ribbon-tool-scripts = Scripts
ribbon-tool-reload = Reload
ribbon-tool-folder = Folder

## Plugin Command-Line Messages
cmd-loaded = OpenCAD LISP loaded ({ $scripts } .lsp scripts in { $dir }, { $commands } c:commands registered)
cmd-reloaded = OpenCAD LISP runner reloaded: { $scripts } .lsp scripts in { $dir }, { $commands } new c:commands registered
cmd-reload-failed = Failed to reload scripts directory: { $error }
cmd-scripts-dir = Scripts directory: { $dir }
cmd-opening-ide = Opening OpenCAD LISP IDE (OCIDE)...
cmd-usage-lsprun = Usage: LSPRUN <lisp_code> or (command ...)
cmd-usage-lsp = Usage: LSP <script_name_or_file> or APPLOAD [<path>]
cmd-registered-commands = Registered OpenCAD LISP commands ({ $count }):
cmd-no-scripts-found = No OpenCAD LISP (.lsp) files found in { $dir }
cmd-available-scripts = Available OpenCAD LISP (.lsp) files ({ $count }):
cmd-no-errors = No OpenCAD LISP errors recorded in this session.
cmd-last-error-title = Last OpenCAD LISP Error
cmd-python-shelved = Python support is currently shelved. Use OpenCAD LISP (LSP, APPLOAD, LSPRUN, or direct S-expressions).

## APPLOAD & STARTUP Dialogs
appload-title = Load/Unload Applications (APPLOAD)
appload-startup-title = Startup Scripts
appload-btn-ok = OK
appload-btn-cancel = Cancel
appload-filter-lisp = OpenCAD LISP Files (*.lsp;*.lisp)
appload-section-load = Load into Current Drawing
appload-label-load-file = OpenCAD LISP File to Load:
appload-checkbox-add-startup = Also add this file to Startup Scripts (auto-load in every drawing)
appload-section-startup = Startup Scripts (Auto-loaded with each new drawing)
appload-label-pick-startup = Pick a script to add to Startup Scripts:
appload-info-auto-add = Selected scripts above are automatically added to the list below.
appload-table-startup-only = Startup Scripts List (Auto-loaded in every drawing):
appload-table-remembered = Remembered Startup Scripts:
appload-col-path = Auto-Loaded File Path
appload-status-added = Added "{ $file }" to startup scripts.
appload-status-removed = Removed "{ $file }" from startup scripts.
appload-err-not-found = APPLOAD: Startup script not found: { $path }
appload-err-load-failed = OpenCAD LISP Load Error in { $path }: { $error }
appload-err-save-failed = APPLOAD: Failed to save Startup Scripts: { $error }
appload-info-updated = { $count ->
    [one] APPLOAD: Startup Scripts updated ({ $count } file remembered).
   *[other] APPLOAD: Startup Scripts updated ({ $count } files remembered).
}

## OCIDE Window & Welcome
ide-window-title = OpenCAD LISP IDE (OCIDE) - { $tab }
ide-welcome = === OpenCAD LISP IDE (OCIDE) ===
    Type OpenCAD LISP code above and click 'Load Script' (F5) or 'Load Selection' (F8).
    Use the REPL input line below for interactive OpenCAD LISP evaluation.

## OCIDE Toolbar
toolbar-continue-tip = Continue (F5)
toolbar-step-over-tip = Step Over (F10)
toolbar-step-into-tip = Step Into (F11)
toolbar-step-out-tip = Step Out (Shift+F11)
toolbar-restart-tip = Restart (Ctrl+Shift+F5)
toolbar-restart-rerun-tip = Restart / Re-run (Ctrl+Shift+F5)
toolbar-stop-tip = Stop (Shift+F5)
toolbar-debugging-badge = ● DEBUGGING
toolbar-load-script-tip = Load Script (F5)
toolbar-load-selection-tip = Load Selection (F8)
toolbar-start-debugging-tip = Start Debugging
toolbar-new-tab-tip = New Script Tab
toolbar-open = Open
toolbar-open-tip = Open Script File
toolbar-save = Save
toolbar-save-tip = Save Script File (Ctrl+S)
toolbar-save-as = Save As...
toolbar-save-as-tip = Save Script As New File (Ctrl+Shift+S)
toolbar-undo-tip = Undo (Ctrl+Z)
toolbar-redo-tip = Redo (Ctrl+Y / Ctrl+Shift+Z)
toolbar-format = Format
toolbar-format-tip = Format OpenCAD LISP Document (Shift+Alt+F) | Selection (Ctrl+Shift+F)
toolbar-find = Find
toolbar-find-tip = Find (Ctrl+F) | Replace (Ctrl+H)
toolbar-variables = Variables
toolbar-variables-tip = Toggle Variables Panel
toolbar-function-list = λ Function List
toolbar-function-list-tip = Toggle OpenCAD LISP Function List & Reference (F2)
toolbar-clear-output = Clear Output
toolbar-clear-output-tip = Clear Output Console
toolbar-switch-to-cad = Switch to CAD
toolbar-transparency-tip = Toggle Window Transparency
toolbar-settings = Settings
toolbar-settings-tip = Open IDE Settings
toolbar-help = Help ▾
toolbar-help-tip = Help & Tutorials
toolbar-tutorials = Tutorials

## Editor, Context Menu & Autocomplete
editor-no-open-file = No open file
editor-no-active-file = No active file
ctx-copy = Copy (Ctrl+C)
ctx-paste = Paste (Ctrl+V)
ctx-find = Find (Ctrl+F)
ctx-replace = Replace (Ctrl+H)
ctx-format-selection = Format Selection (Ctrl+Shift+F)
ctx-format-document = Format Document (Shift+Alt+F)
ctx-add-watch = Add to Watch (Shift+F9)
ctx-add-watch-ident = Add to Watch: '{ $ident }' (Shift+F9)
ctx-toggle-bp-preview = Toggle Breakpoint: { $preview } (F9)
ctx-toggle-bp-line = Toggle Breakpoint (Line { $line }, F9)
autocomplete-custom = Custom
autocomplete-navigate = ↑/↓ Navigate
autocomplete-complete = Tab Complete

## Find & Replace Overlay
find-placeholder = Find...
find-replace-placeholder = Replace with...
find-no-results = No results
find-match-of = { $current } of { $total }
find-match-count = { $count ->
    [one] { $count } match
   *[other] { $count } matches
}
find-btn-replace = Replace
find-btn-all = All

## Status Bar
status-paused-span = ▶ PAUSED at Ln { $line }, Col { $col } { $expr }  |  F5: Continue  |  F10: Step Over  |  F11: Step Into  |  Shift+F11: Step Out  |  Shift+F5: Stop  |  { $count ->
    [one] { $count } breakpoint
   *[other] { $count } breakpoints
}
status-paused-line = ▶ PAUSED at Line { $line }  |  F5: Continue  |  F10: Step Over  |  F11: Step Into  |  Shift+F11: Step Out  |  Shift+F5: Stop  |  { $count ->
    [one] { $count } breakpoint
   *[other] { $count } breakpoints
}
status-debugging-active = 🐞 DEBUGGING active  |  { $count ->
    [one] { $count } breakpoint
   *[other] { $count } breakpoints
} (F9: Toggle BP)
status-breakpoints-list = Breakpoints: [{ $list }]
status-no-breakpoints = No breakpoints
status-ready = Ln { $line }, Col { $col }  |  { $lines ->
    [one] { $lines } line
   *[other] { $lines } lines
}  |  { $bps }  |  OpenCAD LISP  |  F5: Run, F9: Breakpoint, Shift+Alt+F: Format

## Console & REPL
console-title = Console / Output
console-repl-hint = Interactive OpenCAD LISP REPL (Enter to run, Ctrl+Enter for newline)
repl-placeholder-lisp = OpenCAD LISP code / expression (e.g. (+ 1 2) or (getpoint) - Enter to run, Ctrl+Enter for newline)...
repl-placeholder-python = Python code / expression (Enter to run, Ctrl+Enter for newline)...
repl-btn-run = Run
repl-btn-run-lisp-tip = Run OpenCAD LISP Code (Enter to run, Ctrl+Enter for newline)
repl-btn-run-python-tip = Run Python Code (Enter to run, Ctrl+Enter for newline)
repl-btn-clear = Clear
repl-btn-clear-tip = Clear REPL Input

## Debug Inspector Drawer
debug-inspector-title = Debug Inspector
debug-watch-title = Watch Expressions
debug-col-expression = Expression
debug-col-value = Value
debug-btn-clear = Clear
debug-watch-empty = No watch expressions (Right-click or Shift+F9 on variable)
debug-not-in-scope = <not in scope>
debug-not-debugging = <not debugging>
debug-locals-title = Local Variables
debug-col-name = Name
debug-col-type = Type
debug-locals-empty-debugging = No local variables in frame
debug-locals-empty-idle = Start debugging to inspect variables
debug-stack-title = Call Stack
debug-col-function = Function
debug-col-line = Line
debug-col-file = File
debug-stack-empty-debugging = Call stack empty
debug-stack-empty-idle = No active call stack
debug-breakpoints-title = Breakpoints
debug-breakpoints-empty = No breakpoints set (Press F9 on '(' or S-expr)

## API Reference Drawer
api-drawer-title = λ OpenCAD LISP Function List & Reference
api-search-placeholder = Search OpenCAD LISP & DXF API...
api-btn-insert = + Insert
api-active-prefix = λ Active: { $name }
api-active-none = Active Enclosure: (None at cursor)

## Settings Dialog
settings-title = ⚙ IDE Settings
settings-btn-done = Done
settings-tab-general = ⚙ General & Debugger
settings-tab-editor = 🎨 Editor, Font & Syntax
settings-tab-formatting = 📐 OpenCAD LISP Formatting
settings-language-title = Interface Language
settings-language-label = Language:
settings-language-note = Changes apply immediately to OCIDE. Restart OpenCAD Studio to refresh ribbon labels.
settings-bp-color-title = Breakpoint Line Color
settings-exec-color-title = Current Execution Line Color
settings-transparency-label = Transparency Level:
settings-debugger-automation-title = Debugger Automation
settings-auto-transparency = Automatically switch IDE to transparent when debugging (and restore when done)
settings-scripts-dir-title = Default Scripts Location
settings-btn-browse = 📂 Browse...
settings-btn-reset = ↺ Reset
settings-editor-font-label = Editor Font:
settings-font-size-label = Size:
settings-btn-default-font = ↺ Default Font
settings-theme-label = Theme:
settings-theme-new-placeholder = New theme name...
settings-btn-save = 💾 Save
settings-btn-reset-defaults = ↺ Reset All Defaults
settings-syntax-elements-title = Syntax Elements
settings-syntax-bg = Editor Background
settings-syntax-default-text = Default Text
settings-syntax-comments = Comments (;)
settings-syntax-comments-short = Comments
settings-syntax-strings = Strings ("...")
settings-syntax-strings-short = Strings
settings-syntax-keywords = Keywords & Forms
settings-syntax-native-funcs = Native Functions
settings-syntax-custom-funcs = Custom Functions
settings-syntax-user-custom-funcs = User Custom Functions
settings-syntax-numbers = Numbers
settings-syntax-parens = Parentheses ()
settings-syntax-parens-short = Parentheses
settings-adjust-color = Adjust Color: { $target }
settings-hex-label = Hex:
settings-btn-apply = Apply
settings-quick-palette = Quick Palette:
settings-custom-funcs-empty = No custom functions added yet.
settings-custom-funcs-placeholder = New function name...
settings-btn-add = + Add
settings-btn-remove = − Remove
settings-custom-funcs-selected = Selected: { $name }
settings-custom-funcs-select-hint = Select an item in list to remove
settings-color-mode-label = Color Mode:
settings-color-mode-native = Same as Native
settings-color-mode-dedicated = Dedicated Custom Color
settings-manage-func-names = Manage Function Names:
settings-live-preview-title = Live Syntax Preview
settings-formatting-title = OpenCAD LISP Formatting Guidelines
settings-formatting-subtitle = Configure automatic indentation, symbol casing, and alignment rules matching classic LISP and standard CAD specifications.
settings-indent-label = Indentation Size:
settings-indent-2 = 2 spaces (default)
settings-indent-3 = 3 spaces
settings-indent-4 = 4 spaces
settings-reflow-label = Structural Re-flow: Move expressions between lines (classic LISP breaks & joins)
settings-reflow-note = Expands defun bodies, progn, while/repeat loops, multi-pair setq, and cond clauses onto separate lines; condenses fragmented atoms/parentheses into single lines.
settings-downcase-label = Convert built-in keywords and functions to lowercase (defun, setq, getpoint, princ, etc.)
settings-downcase-note = Standardizes language keywords and built-in functions to lowercase, and boolean constants to T and nil, while preserving original casing for user variables and strings.
settings-wide-label = Wide Style: Align multi-line function call arguments under the first argument
settings-wide-note = For regular function calls like (command "_.LINE" ...) or (strcat ...), subsequent lines align vertically under the first argument on line 1.
settings-align-setq-label = Align variable-value pairs in (setq ...) blocks
settings-align-setq-note = In multi-line (setq ...) assignments, subsequent variable names align vertically under the first variable.
settings-shortcuts-title = Keyboard & Context Shortcuts:
settings-shortcut-format-doc-label = • Format Entire Document:
settings-shortcut-format-doc-keys = Shift+Alt+F  or  Ctrl+Alt+F (default)
settings-shortcut-format-sel-label = • Format Selection:
settings-shortcut-format-sel-keys = Ctrl+Shift+F  or  Right-Click → Format Selection
settings-shortcut-smart-enter-label = • Smart Indent on Enter:
settings-shortcut-smart-enter-desc = Enter automatically computes indentation based on current rules
settings-shortcut-comments-label = • Comments:
settings-shortcut-comments-desc = ;;; stays at column 0  |  ;; aligns to code block
settings-btn-format-now = ⚡ Format Active Document Now

## Save Confirmation Dialog
save-dialog-title = 💾 Unsaved Changes
save-dialog-prompt-single = Do you want to save changes to "{ $file }"?
save-dialog-prompt-multi = Do you want to save changes to { $count } files before closing?
save-dialog-warning = Your changes will be lost if you don't save them.
save-dialog-btn-save = Save
save-dialog-btn-dont-save = Don't Save
save-dialog-btn-cancel = Cancel

## Find & Replace Tooltips
find-toggle-replace-tip = Toggle Replace
find-prev-tip = Previous Match (Shift+F3)
find-next-tip = Next Match (F3)
find-case-tip = Match Case
find-word-tip = Match Whole Word
find-close-tip = Close (Esc)

## OCIDE File Dialog Filters & Console Status Messages
dialog-filter-lisp = OpenCAD LISP Routine (*.lsp, *.lisp)
dialog-filter-all = All Files (*.*)
console-debug-paused = [DEBUG] Paused at { $file }:{ $line }:{ $col }
console-debug-paused-expr = [DEBUG] Paused at { $file }:{ $line }:{ $col } -> { $expr }
console-debug-finished = [DEBUG] Execution finished.
console-debug-error = [DEBUG ERROR] { $error }
console-cad-selection = { $count ->
    [one] [CAD Selection] { $count } entity selected: { $handles }
   *[other] [CAD Selection] { $count } entities selected: { $handles }
}
console-ide-opened = [IDE] Opened { $path }
console-ide-saved = [IDE] Saved { $file }
console-repl-mode-switched = [REPL mode switched to { $mode }]
console-cleared = === Console Cleared ===
console-loading-script = --- Loading Script: { $file } ---
console-tutorials-open-failed = Failed to open tutorials ({ $path }): { $error }
console-tutorials-not-found = Tutorials index.html not found.

## Settings Color Swatch Tooltips
color-dark-red-default = Dark Red (Default)
color-crimson = Crimson
color-burgundy = Burgundy
color-purple = Purple
color-orange = Orange
color-navy-blue = Navy Blue
color-charcoal = Charcoal
color-teal = Teal
color-green-default = Green (Default)
color-emerald = Emerald
color-lime = Lime
color-yellow-gold = Yellow / Gold
color-cyan = Cyan
color-sky-blue = Sky Blue
color-mocha-default = Base16 Mocha Default
color-pure-white = Pure White (#FFFFFF)
color-soft-cream = Soft Cream (#F8F8F2)
color-light-gray = Light Gray (#D0C8C6)
color-muted-gray = Muted Gray (#839496)
color-charcoal-hex = Charcoal (#272822)
color-mocha-bg = Mocha BG (#202225)
color-pure-black = Pure Black (#000000)
color-solarized-dark = Solarized Dark (#002B36)
color-red-coral = Red / Coral (#FF5555)
color-orange-hex = Orange (#FFB86C)
color-gold-yellow = Gold / Yellow (#F1FA8C)
color-emerald-green = Emerald Green (#50FA7B)
color-cyan-teal = Cyan / Teal (#8BE9FD)
color-classic-blue = Classic Blue (#0000FF)
color-navy-hex = Navy (#000080)
color-sky-blue-hex = Sky Blue (#82AAFF)
color-purple-lavender = Purple / Lavender (#BD93F9)
color-magenta = Magenta (#FF79C6)
color-classic-purple = Classic Purple (#800080)
color-sienna-brown = Sienna Brown (#A0522D)
