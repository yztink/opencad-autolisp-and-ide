//! OpenCAD LISP, Extended LISP, Object Model, and OpenCAD Native UI API Reference Catalog.
//!
//! Provides function signatures, required vs [optional] parameter conventions,
//! descriptions, snippets, and minimum required DXF group codes for CAD entities.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiDoc {
    pub name: &'static str,
    pub signature: &'static str,
    pub description: &'static str,
    pub snippet: &'static str,
    pub category: &'static str,
    pub dxf_summary: Option<&'static str>,
}

pub const ENTITY_DXF_SUMMARY: &str = "\
Minimum DXF Codes by Entity:
• LINE:       (0 . \"LINE\") (10 x y z) (11 x y z)
• CIRCLE:     (0 . \"CIRCLE\") (10 x y z) (40 . radius)
• ARC:        (0 . \"ARC\") (10 x y z) (40 . radius) (50 . start_deg) (51 . end_deg)
• LWPOLYLINE: (0 . \"LWPOLYLINE\") (70 . flag) (90 . n) (10 x y)... [(42 . bulge)]
• TEXT:       (0 . \"TEXT\") (1 . \"str\") (10 x y z) (40 . height) [(50 . rot)]
• MTEXT:      (0 . \"MTEXT\") (1 . \"str\") (10 x y z) (40 . height)
• POINT:      (0 . \"POINT\") (10 x y z)
• SOLID:      (0 . \"SOLID\") (10 p1) (11 p2) (12 p3) (13 p4)
• INSERT:     (0 . \"INSERT\") (2 . \"BLOCK\") (10 x y z) [(41 . sx) (42 . sy) (50 . rot)]
• ELLIPSE:    (0 . \"ELLIPSE\") (10 center) (11 major_axis) (40 . ratio)
• -3 XDATA:   (-3 (\"APPID\" (1000 . \"str\") (1040 . real) (1070 . int) (1010 x y z)))";

pub const ENTMOD_DXF_SUMMARY: &str = "\
Minimum DXF Codes for In-Place Modification:
• (5 . \"HANDLE\") or -1 ename — Target entity identifier (required)
• (0 . \"TYPE\") — Entity type verification
• Updated properties only:
  - (10 x y z) / (11 x y z) — Move points
  - (40 . radius_or_height) — Resize circle, arc, or text
  - (8 . \"LAYER\") — Change layer assignment
  - (62 . aci_color) — Change ACI color index (1=Red, 2=Yellow, etc.)
  - (-3 (\"APPID\" ...)) — Attach or update extended entity data (XDATA)";

pub const API_DOCS: &[ApiDoc] = &[
    // ── 1. CAD Database & Entity Manipulation (ent*) ────────────────────────
    ApiDoc {
        name: "entmake",
        signature: "(entmake dxf-alist) -> dxf-alist | nil",
        description: "Creates a new CAD entity from a DXF association list. Returns the alist if successful, nil on error.",
        snippet: "(entmake (list '(0 . \"CIRCLE\") (cons 10 '(0.0 0.0 0.0)) (cons 40 25.0) '(8 . \"0\")))\n",
        category: "CAD Database",
        dxf_summary: Some(ENTITY_DXF_SUMMARY),
    },
    ApiDoc {
        name: "entmakex",
        signature: "(entmakex dxf-alist) -> ename | nil",
        description: "Creates a new CAD entity from a DXF association list and returns its entity name (ENAME) directly.",
        snippet: "(setq e (entmakex (list '(0 . \"LINE\") (cons 10 '(0.0 0.0 0.0)) (cons 11 '(100.0 50.0 0.0))))))\n",
        category: "CAD Database",
        dxf_summary: Some(ENTITY_DXF_SUMMARY),
    },
    ApiDoc {
        name: "entmod",
        signature: "(entmod dxf-alist) -> dxf-alist | nil",
        description: "Modifies an existing entity in the drawing database using an updated DXF association list.",
        snippet: "(setq ent (entget (entlast)))\n(entmod (subst '(40 . 50.0) (assoc 40 ent) ent))\n",
        category: "CAD Database",
        dxf_summary: Some(ENTMOD_DXF_SUMMARY),
    },
    ApiDoc {
        name: "entget",
        signature: "(entget ename [app-list]) -> dxf-alist",
        description: "Retrieves the DXF definition data of an entity. If app-list is supplied, includes registered -3 XDATA.",
        snippet: "(setq data (entget (entlast)))\n(setq rad (cdr (assoc 40 data)))\n",
        category: "CAD Database",
        dxf_summary: None,
    },
    ApiDoc {
        name: "entdel",
        signature: "(entdel ename) -> ename | nil",
        description: "Deletes the specified entity from the drawing or un-deletes it if previously deleted.",
        snippet: "(entdel (entlast))\n",
        category: "CAD Database",
        dxf_summary: None,
    },
    ApiDoc {
        name: "entlast",
        signature: "(entlast) -> ename | nil",
        description: "Returns the entity name (ENAME) of the last non-deleted main entity created in the drawing.",
        snippet: "(setq last-ent (entlast))\n",
        category: "CAD Database",
        dxf_summary: None,
    },
    ApiDoc {
        name: "entnext",
        signature: "(entnext [ename]) -> ename | nil",
        description: "Returns the first non-deleted entity name in the drawing, or the entity following the specified ename.",
        snippet: "(setq cur (entnext))\n(while cur\n  (princ (cdr (assoc 0 (entget cur))))\n  (setq cur (entnext cur))\n)\n",
        category: "CAD Database",
        dxf_summary: None,
    },
    ApiDoc {
        name: "entupd",
        signature: "(entupd ename) -> ename",
        description: "Refreshes the on-screen display of an entity after modification with entmod.",
        snippet: "(entupd (entlast))\n",
        category: "CAD Database",
        dxf_summary: None,
    },
    ApiDoc {
        name: "handent",
        signature: "(handent handle-str) -> ename | nil",
        description: "Finds an entity name (ENAME) associated with a persistent hexadecimal handle string.",
        snippet: "(setq ent (handent \"2B\"))\n",
        category: "CAD Database",
        dxf_summary: None,
    },
    ApiDoc {
        name: "regapp",
        signature: "(regapp application-name) -> application-name | nil",
        description: "Registers an extended data (XDATA) application name in the drawing database.",
        snippet: "(regapp \"MY_APP_NAME\")\n",
        category: "CAD Database",
        dxf_summary: None,
    },

    // ── 2. CAD Commands & Host Control ──────────────────────────────────────
    ApiDoc {
        name: "command",
        signature: "(command [cmd-token1 ...]) -> nil",
        description: "Executes CAD host commands step-by-step. Dual-mode commands automatically run their command-line (-) variant unless (initdia 1) is called first. Supports point lists, strings, PAUSE, and \"\" for Enter.",
        snippet: "(command \"_.LINE\" '(0 0 0) '(100 50 0) \"\")\n",
        category: "CAD Commands",
        dxf_summary: None,
    },
    ApiDoc {
        name: "command-s",
        signature: "(command-s [cmd-token1 ...]) -> nil",
        description: "Synchronously executes CAD host commands using command-line mode by default unless (initdia 1) is called first.",
        snippet: "(command-s \"_.LAYER\" \"_M\" \"Walls\" \"\")\n",
        category: "CAD Commands",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-cmdf",
        signature: "(vl-cmdf [cmd-token1 ...]) -> T | nil",
        description: "Evaluates all arguments and executes CAD host commands step-by-step, returning T when completed. Dual-mode commands run their command-line (-) variant unless (initdia 1) is called.",
        snippet: "(vl-cmdf \"_.INSERT\" \"MyBlock\" '(0 0 0) 1 1 0)\n",
        category: "CAD Commands",
        dxf_summary: None,
    },
    ApiDoc {
        name: "initdia",
        signature: "(initdia [dialogflag]) -> nil",
        description: "Forces the next CAD command invoked by (command ...) to display its GUI dialog box when dialogflag is omitted or non-zero, or suppresses it when 0 or nil.",
        snippet: "(initdia 1)\n(command \"LAYER\")\n",
        category: "CAD Commands",
        dxf_summary: None,
    },
    ApiDoc {
        name: "getvar",
        signature: "(getvar var-name) -> value",
        description: "Reads the current value of an OpenCAD Studio system variable (e.g. \"CLAYER\", \"OSMODE\").",
        snippet: "(setq cur-layer (getvar \"CLAYER\"))\n",
        category: "CAD Commands",
        dxf_summary: None,
    },
    ApiDoc {
        name: "setvar",
        signature: "(setvar var-name value) -> value",
        description: "Updates an OpenCAD Studio system variable (e.g. \"CLAYER\", \"OSMODE\").",
        snippet: "(setvar \"CLAYER\" \"0\")\n",
        category: "CAD Commands",
        dxf_summary: None,
    },
    ApiDoc {
        name: "alert",
        signature: "(alert message-str) -> nil",
        description: "Displays an informational dialog box with an OK button to the user.",
        snippet: "(alert \"Operation completed successfully!\")\n",
        category: "CAD Commands",
        dxf_summary: None,
    },
    ApiDoc {
        name: "add-line",
        signature: "(add-line pt1 pt2 [layer]) -> ename",
        description: "Direct host API helper: creates a line entity between pt1 and pt2 on optional layer.",
        snippet: "(add-line '(0.0 0.0 0.0) '(100.0 50.0 0.0) \"0\")\n",
        category: "CAD Commands",
        dxf_summary: None,
    },
    ApiDoc {
        name: "add-circle",
        signature: "(add-circle center radius [layer]) -> ename",
        description: "Direct host API helper: creates a circle entity at center with radius on optional layer.",
        snippet: "(add-circle '(50.0 50.0 0.0) 25.0 \"0\")\n",
        category: "CAD Commands",
        dxf_summary: None,
    },

    // ── 3. Interactive User Prompts ─────────────────────────────────────────
    ApiDoc {
        name: "getpoint",
        signature: "(getpoint [base-pt] [prompt-str]) -> (x y z) | nil",
        description: "Prompts the user to pick a coordinate point in the viewport with optional rubber-band line.",
        snippet: "(setq pt (getpoint \"\\nSpecify insertion point: \"))\n",
        category: "Interactive",
        dxf_summary: None,
    },
    ApiDoc {
        name: "getcorner",
        signature: "(getcorner base-pt [prompt-str]) -> (x y z) | nil",
        description: "Prompts the user to pick the opposite corner of a rectangle from base-pt with live rubber-band box.",
        snippet: "(setq p1 (getpoint \"\\nPick first corner: \"))\n(setq p2 (getcorner p1 \"\\nPick opposite corner: \"))\n",
        category: "Interactive",
        dxf_summary: None,
    },
    ApiDoc {
        name: "getdist",
        signature: "(getdist [base-pt] [prompt-str]) -> real | nil",
        description: "Prompts the user to specify a distance by entering a number or picking two points.",
        snippet: "(setq dist (getdist pt \"\\nSpecify offset distance: \"))\n",
        category: "Interactive",
        dxf_summary: None,
    },
    ApiDoc {
        name: "getangle",
        signature: "(getangle [base-pt] [prompt-str]) -> real | nil",
        description: "Prompts the user for an angle in radians measured counter-clockwise from current 0-angle.",
        snippet: "(setq ang (getangle pt \"\\nSpecify rotation angle: \"))\n",
        category: "Interactive",
        dxf_summary: None,
    },
    ApiDoc {
        name: "getorient",
        signature: "(getorient [base-pt] [prompt-str]) -> real | nil",
        description: "Prompts the user for an absolute orientation angle in radians (0 always east).",
        snippet: "(setq ang (getorient pt \"\\nSpecify orientation: \"))\n",
        category: "Interactive",
        dxf_summary: None,
    },
    ApiDoc {
        name: "getstring",
        signature: "(getstring [has-spaces-flag] [prompt-str]) -> string",
        description: "Prompts the user to enter a text string at the command line. If has-spaces-flag is T, spaces are allowed.",
        snippet: "(setq name (getstring T \"\\nEnter part label: \"))\n",
        category: "Interactive",
        dxf_summary: None,
    },
    ApiDoc {
        name: "getint",
        signature: "(getint [prompt-str]) -> integer | nil",
        description: "Prompts the user to enter a whole integer value.",
        snippet: "(setq count (getint \"\\nEnter number of items: \"))\n",
        category: "Interactive",
        dxf_summary: None,
    },
    ApiDoc {
        name: "getreal",
        signature: "(getreal [prompt-str]) -> real | nil",
        description: "Prompts the user to enter a floating-point real number.",
        snippet: "(setq scale (getreal \"\\nEnter scale factor <1.0>: \"))\n",
        category: "Interactive",
        dxf_summary: None,
    },
    ApiDoc {
        name: "getkword",
        signature: "(getkword [prompt-str]) -> string | nil",
        description: "Prompts the user for one of the keywords previously established by (initget).",
        snippet: "(initget 1 \"Yes No\")\n(setq ans (getkword \"\\nProceed? [Yes/No]: \"))\n",
        category: "Interactive",
        dxf_summary: None,
    },
    ApiDoc {
        name: "initget",
        signature: "(initget [bits] [keywords-str]) -> nil",
        description: "Initializes input constraints (bits: 1=no null, 2=no zero, 4=no negative) and keywords for next get* call.",
        snippet: "(initget (+ 1 2 4) \"Red Green Blue\")\n",
        category: "Interactive",
        dxf_summary: None,
    },

    // ── 4. Selection Sets (ss*) & Entity Picking ────────────────────────────
    ApiDoc {
        name: "entsel",
        signature: "(entsel [prompt-str]) -> (ename (x y z)) | nil",
        description: "Prompts the user to select a single entity in the viewport and returns a list containing the entity name (ENAME) and the 3D pick point coordinates, or nil on Enter/cancel.",
        snippet: "(if (setq sel (entsel \"\\nSelect object: \"))\n  (progn\n    (setq ent (car sel))\n    (setq pt  (cadr sel))\n    (entget ent)\n  )\n)\n",
        category: "Selection Sets",
        dxf_summary: None,
    },
    ApiDoc {
        name: "nentsel",
        signature: "(nentsel [prompt-str]) -> (ename (x y z)) | nil",
        description: "Prompts the user to select a single entity or nested subentity in the viewport and returns its entity name and pick point coordinates, or nil on cancel.",
        snippet: "(if (setq sel (nentsel \"\\nSelect entity: \"))\n  (entget (car sel))\n)\n",
        category: "Selection Sets",
        dxf_summary: None,
    },
    ApiDoc {
        name: "nentselp",
        signature: "(nentselp [prompt-str] [pt]) -> (ename (x y z)) | nil",
        description: "Prompts the user to select an entity or nested subentity (with optional prompt string and pick point) and returns its entity name and pick point, or nil.",
        snippet: "(if (setq sel (nentselp \"\\nSelect entity: \"))\n  (car sel)\n)\n",
        category: "Selection Sets",
        dxf_summary: None,
    },
    ApiDoc {
        name: "ssget",
        signature: "(ssget [mode] [pt1 [pt2]] [filter-alist]) -> pickset | nil",
        description: "Creates a selection set of entities matching interactive picks, windows (\"W\"), crossing (\"C\"), or \"X\" with DXF filter.",
        snippet: "(setq ss (ssget \"X\" '((0 . \"CIRCLE\") (8 . \"WALLS\"))))\n",
        category: "Selection Sets",
        dxf_summary: None,
    },
    ApiDoc {
        name: "sslength",
        signature: "(sslength pickset) -> integer",
        description: "Returns the number of entity names contained in selection set pickset.",
        snippet: "(setq count (sslength ss))\n",
        category: "Selection Sets",
        dxf_summary: None,
    },
    ApiDoc {
        name: "ssname",
        signature: "(ssname pickset index) -> ename | nil",
        description: "Returns the entity name (ENAME) at 0-based index in the selection set.",
        snippet: "(setq ent (ssname ss 0))\n",
        category: "Selection Sets",
        dxf_summary: None,
    },
    ApiDoc {
        name: "ssadd",
        signature: "(ssadd [ename [pickset]]) -> pickset",
        description: "Creates an empty selection set, or adds ename to an existing selection set.",
        snippet: "(setq ss (ssadd (entlast) ss))\n",
        category: "Selection Sets",
        dxf_summary: None,
    },
    ApiDoc {
        name: "ssdel",
        signature: "(ssdel ename pickset) -> pickset | nil",
        description: "Removes an entity name from a selection set.",
        snippet: "(ssdel (entlast) ss)\n",
        category: "Selection Sets",
        dxf_summary: None,
    },
    ApiDoc {
        name: "ssmemb",
        signature: "(ssmemb ename pickset) -> ename | nil",
        description: "Tests whether an entity name belongs to the selection set. Returns ename if present, nil otherwise.",
        snippet: "(if (ssmemb ent ss) (princ \"\\nIn selection\"))\n",
        category: "Selection Sets",
        dxf_summary: None,
    },

    // ── 5. Special Forms & Flow Control ─────────────────────────────────────
    ApiDoc {
        name: "defun",
        signature: "(defun name ([param ...] [/ local ...]) expr ...) -> symbol",
        description: "Defines a function. Prefixing name with c: (e.g. c:mycmd) exposes it as a top-level CAD command.",
        snippet: "(defun c:drawbox (/ p1 p2)\n  (setq p1 (getpoint \"\\nFirst corner: \"))\n  (setq p2 (getcorner p1 \"\\nOther corner: \"))\n  (command \"_.RECTANG\" p1 p2)\n  (princ)\n)\n",
        category: "Control Flow",
        dxf_summary: None,
    },
    ApiDoc {
        name: "setq",
        signature: "(setq sym1 expr1 [sym2 expr2 ...]) -> value",
        description: "Assigns the result of evaluating each expression to its preceding symbol and returns the last value.",
        snippet: "(setq r 25.0\n      area (* pi r r))\n",
        category: "Control Flow",
        dxf_summary: None,
    },
    ApiDoc {
        name: "set",
        signature: "(set sym-expr val-expr) -> value",
        description: "Evaluates sym-expr to obtain a symbol and assigns val-expr to it.",
        snippet: "(set (read \"MY_VAR\") 42)\n",
        category: "Control Flow",
        dxf_summary: None,
    },
    ApiDoc {
        name: "quote",
        signature: "(quote expr) -> expr",
        description: "Returns expr unevaluated without computing it. Abbreviated as 'expr.",
        snippet: "'(10 20 30)\n",
        category: "Control Flow",
        dxf_summary: None,
    },
    ApiDoc {
        name: "lambda",
        signature: "(lambda ([param ...] [/ local ...]) expr ...) -> closure",
        description: "Creates an anonymous function closure, commonly used with mapcar, apply, or vl-remove-if.",
        snippet: "(mapcar '(lambda (x) (* x 2)) '(1 2 3 4))\n",
        category: "Control Flow",
        dxf_summary: None,
    },
    ApiDoc {
        name: "if",
        signature: "(if test-expr then-expr [else-expr]) -> value",
        description: "Evaluates test-expr. If non-nil, returns then-expr; otherwise evaluates and returns optional else-expr.",
        snippet: "(if (> radius 0.0)\n  (princ \"\\nValid radius\")\n  (princ \"\\nInvalid radius\")\n)\n",
        category: "Control Flow",
        dxf_summary: None,
    },
    ApiDoc {
        name: "cond",
        signature: "(cond (test1 expr ...) [(test2 expr ...) ...]) -> value",
        description: "Evaluates test clauses in sequence until one is non-nil, then evaluates and returns that clause's expressions.",
        snippet: "(cond\n  ((= typ \"LINE\") (princ \"\\nLine\"))\n  ((= typ \"CIRCLE\") (princ \"\\nCircle\"))\n  (t (princ \"\\nOther\"))\n)\n",
        category: "Control Flow",
        dxf_summary: None,
    },
    ApiDoc {
        name: "while",
        signature: "(while test-expr body-expr ...) -> value",
        description: "Repeatedly executes body-expr expressions as long as test-expr evaluates to non-nil.",
        snippet: "(setq i 0)\n(while (< i 10)\n  (princ i)\n  (setq i (1+ i))\n)\n",
        category: "Control Flow",
        dxf_summary: None,
    },
    ApiDoc {
        name: "repeat",
        signature: "(repeat count-int body-expr ...) -> value",
        description: "Executes body-expr expressions count times.",
        snippet: "(repeat 5\n  (princ \"*\")\n)\n",
        category: "Control Flow",
        dxf_summary: None,
    },
    ApiDoc {
        name: "foreach",
        signature: "(foreach var list body-expr ...) -> value",
        description: "Iterates through each element in list, binding var to each item and evaluating body-expr.",
        snippet: "(foreach item '(\"A\" \"B\" \"C\")\n  (princ item)\n)\n",
        category: "Control Flow",
        dxf_summary: None,
    },
    ApiDoc {
        name: "progn",
        signature: "(progn body-expr ...) -> value",
        description: "Evaluates multiple expressions sequentially and returns the result of the final expression.",
        snippet: "(if flag\n  (progn\n    (princ \"\\nTrue: \")\n    (setq count (1+ count))\n  )\n)\n",
        category: "Control Flow",
        dxf_summary: None,
    },
    ApiDoc {
        name: "and",
        signature: "(and expr ...) -> boolean",
        description: "Evaluates expressions from left to right; returns nil immediately if any evaluates to nil, else returns T.",
        snippet: "(if (and (> x 0) (< x 100)) (princ \"\\nIn range\"))\n",
        category: "Control Flow",
        dxf_summary: None,
    },
    ApiDoc {
        name: "or",
        signature: "(or expr ...) -> boolean",
        description: "Evaluates expressions from left to right; returns T immediately if any is non-nil, else returns nil.",
        snippet: "(if (or (= c \"R\") (= c \"Red\")) (princ \"\\nRed chosen\"))\n",
        category: "Control Flow",
        dxf_summary: None,
    },

    // ── 6. Evaluation, Logic & Predicates ───────────────────────────────────
    ApiDoc {
        name: "eval",
        signature: "(eval expr) -> value",
        description: "Evaluates an expression and returns the result.",
        snippet: "(eval '(+ 10 20))\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "apply",
        signature: "(apply func arg-list) -> value",
        description: "Applies a function to a list of arguments.",
        snippet: "(apply '+ '(1 2 3 4 5))\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "mapcar",
        signature: "(mapcar func list1 [list2 ...]) -> list",
        description: "Applies func element-wise across one or more lists and returns a list of the results.",
        snippet: "(mapcar '+ '(1 2 3) '(10 20 30))\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "boundp",
        signature: "(boundp sym) -> boolean",
        description: "Returns T if symbol has a bound value in current environment, nil otherwise.",
        snippet: "(if (boundp 'my-var) (princ \"\\nBound\"))\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "null",
        signature: "(null expr) -> boolean",
        description: "Returns T if expr evaluates to nil, nil otherwise.",
        snippet: "(if (null ss) (princ \"\\nNothing selected\"))\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "not",
        signature: "(not expr) -> boolean",
        description: "Returns T if expr is nil, nil if expr is non-nil.",
        snippet: "(if (not (= a b)) (princ \"\\nNot equal\"))\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "type",
        signature: "(type expr) -> symbol",
        description: "Returns symbol representing the data type of expr (e.g. REAL, INT, STR, LIST, ENAME, PICKSET).",
        snippet: "(setq t (type 42)) ; => INT\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "=",
        signature: "(= num1 num2 ...) -> boolean",
        description: "Returns T if all numerical or string arguments are equal.",
        snippet: "(= 10 10.0)\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "/=",
        signature: "(/= expr1 expr2 ...) -> boolean",
        description: "Returns T if arguments are not equal.",
        snippet: "(/= a b)\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "<",
        signature: "(< num1 num2 ...) -> boolean",
        description: "Returns T if each argument is strictly less than the following argument.",
        snippet: "(< 1 2 3)\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "<=",
        signature: "(<= num1 num2 ...) -> boolean",
        description: "Returns T if each argument is less than or equal to the following argument.",
        snippet: "(<= 1 1 2)\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: ">",
        signature: "(> num1 num2 ...) -> boolean",
        description: "Returns T if each argument is strictly greater than the following argument.",
        snippet: "(> 3 2 1)\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: ">=",
        signature: "(>= num1 num2 ...) -> boolean",
        description: "Returns T if each argument is greater than or equal to the following argument.",
        snippet: "(>= 2 2 1)\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "eq",
        signature: "(eq expr1 expr2) -> boolean",
        description: "Returns T if expr1 and expr2 are identical (point to identical memory address or atom).",
        snippet: "(eq a a)\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "equal",
        signature: "(equal expr1 expr2 [fuzz]) -> boolean",
        description: "Returns T if expr1 and expr2 evaluate to the same value within optional numerical tolerance fuzz.",
        snippet: "(equal 1.000001 1.0 1e-4)\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "atom",
        signature: "(atom expr) -> boolean",
        description: "Returns T if expr is not a list (or is nil), nil otherwise.",
        snippet: "(atom \"hello\") ; => T\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "listp",
        signature: "(listp expr) -> boolean",
        description: "Returns T if expr is a list or nil, nil otherwise.",
        snippet: "(listp '(1 2))\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "numberp",
        signature: "(numberp expr) -> boolean",
        description: "Returns T if expr is an integer or real number.",
        snippet: "(numberp 3.14)\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "zerop",
        signature: "(zerop num) -> boolean",
        description: "Returns T if num evaluates to zero (integer or float), nil otherwise.",
        snippet: "(zerop 0.0)\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },
    ApiDoc {
        name: "minusp",
        signature: "(minusp num) -> boolean",
        description: "Returns T if num evaluates to a negative number.",
        snippet: "(minusp -5.2)\n",
        category: "Logic & Predicates",
        dxf_summary: None,
    },

    // ── 7. Math & Geometry ──────────────────────────────────────────────────
    ApiDoc {
        name: "+",
        signature: "(+ [num1 num2 ...]) -> number",
        description: "Returns the sum of all arguments.",
        snippet: "(+ 10 20 30)\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "-",
        signature: "(- num1 [num2 ...]) -> number",
        description: "Subtracts subsequent numbers from num1, or negates num1 if only one argument is provided.",
        snippet: "(- 100 25 10)\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "*",
        signature: "(* [num1 num2 ...]) -> number",
        description: "Returns the product of all arguments.",
        snippet: "(* 2 3 4)\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "/",
        signature: "(/ num1 [num2 ...]) -> number",
        description: "Divides num1 by subsequent numbers. Note: integer division produces integer result unless a float is involved.",
        snippet: "(/ 100.0 4.0)\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "1+",
        signature: "(1+ num) -> number",
        description: "Increments num by 1.",
        snippet: "(1+ i)\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "1-",
        signature: "(1- num) -> number",
        description: "Decrements num by 1.",
        snippet: "(1- i)\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "abs",
        signature: "(abs num) -> number",
        description: "Returns the absolute value of num.",
        snippet: "(abs -42.5)\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "max",
        signature: "(max num1 num2 ...) -> number",
        description: "Returns the largest numerical value among arguments.",
        snippet: "(max 10 25 5)\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "min",
        signature: "(min num1 num2 ...) -> number",
        description: "Returns the smallest numerical value among arguments.",
        snippet: "(min 10 25 5)\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "rem",
        signature: "(rem num1 num2) -> number",
        description: "Returns remainder of num1 divided by num2 (modulo).",
        snippet: "(rem 10 3) ; => 1\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "sqrt",
        signature: "(sqrt num) -> real",
        description: "Returns the square root of num.",
        snippet: "(sqrt 144.0)\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "expt",
        signature: "(expt base power) -> number",
        description: "Returns base raised to the power exponent.",
        snippet: "(expt 2 8) ; => 256\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "sin",
        signature: "(sin angle-rad) -> real",
        description: "Returns the sine of an angle specified in radians.",
        snippet: "(sin (/ pi 2.0))\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "cos",
        signature: "(cos angle-rad) -> real",
        description: "Returns the cosine of an angle specified in radians.",
        snippet: "(cos pi)\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "atan",
        signature: "(atan y [x]) -> real",
        description: "Returns arc-tangent of y, or two-argument atan2(y, x) in radians.",
        snippet: "(atan 1.0 1.0) ; => (/ pi 4.0)\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "distance",
        signature: "(distance pt1 pt2) -> real",
        description: "Calculates the 2D or 3D Euclidean distance between two point lists.",
        snippet: "(setq d (distance '(0 0 0) '(100 50 0)))\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "angle",
        signature: "(angle pt1 pt2) -> real",
        description: "Calculates the angle in radians from pt1 to pt2 measured counter-clockwise from the X axis.",
        snippet: "(setq a (angle '(0 0) '(10 10)))\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "polar",
        signature: "(polar pt angle-rad distance) -> (x y [z])",
        description: "Returns a point offset from pt at angle-rad by distance.",
        snippet: "(setq next-pt (polar pt (* 0.5 pi) 50.0))\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },
    ApiDoc {
        name: "inters",
        signature: "(inters pt1 pt2 pt3 pt4 [on-segment-flag]) -> (x y z) | nil",
        description: "Finds the intersection point of two lines. If on-segment-flag is non-nil or omitted, restricts to segments.",
        snippet: "(setq int-pt (inters '(0 0) '(100 100) '(0 100) '(100 0)))\n",
        category: "Math & Geometry",
        dxf_summary: None,
    },

    // ── 8. List Manipulation ────────────────────────────────────────────────
    ApiDoc {
        name: "car",
        signature: "(car list) -> first-elem",
        description: "Returns the first element of a list.",
        snippet: "(car '(10 20 30)) ; => 10\n",
        category: "List Manipulation",
        dxf_summary: None,
    },
    ApiDoc {
        name: "cdr",
        signature: "(cdr list) -> tail-list",
        description: "Returns all elements of list following the first element, or value of a dotted pair.",
        snippet: "(cdr '(10 20 30)) ; => (20 30)\n",
        category: "List Manipulation",
        dxf_summary: None,
    },
    ApiDoc {
        name: "cadr",
        signature: "(cadr list) -> second-elem",
        description: "Returns the second element of list (car of cdr).",
        snippet: "(cadr '(10 20 30)) ; => 20\n",
        category: "List Manipulation",
        dxf_summary: None,
    },
    ApiDoc {
        name: "caddr",
        signature: "(caddr list) -> third-elem",
        description: "Returns the third element of list (car of cdr of cdr).",
        snippet: "(caddr '(10 20 30)) ; => 30\n",
        category: "List Manipulation",
        dxf_summary: None,
    },
    ApiDoc {
        name: "cons",
        signature: "(cons elem list-or-elem) -> list | dotted-pair",
        description: "Prepends elem to list, or constructs a dotted pair (a . b) if second argument is an atom.",
        snippet: "(cons 10 '(20.0 30.0 0.0))\n",
        category: "List Manipulation",
        dxf_summary: None,
    },
    ApiDoc {
        name: "list",
        signature: "(list [elem1 elem2 ...]) -> list",
        description: "Constructs and returns a new list containing the evaluated arguments.",
        snippet: "(list '(0 . \"LINE\") (cons 10 p1) (cons 11 p2))\n",
        category: "List Manipulation",
        dxf_summary: None,
    },
    ApiDoc {
        name: "append",
        signature: "(append [list1 list2 ...]) -> list",
        description: "Combines multiple lists into a single flat list.",
        snippet: "(append '(1 2) '(3 4))\n",
        category: "List Manipulation",
        dxf_summary: None,
    },
    ApiDoc {
        name: "reverse",
        signature: "(reverse list) -> list",
        description: "Returns a copy of list with elements in reverse order.",
        snippet: "(reverse '(1 2 3))\n",
        category: "List Manipulation",
        dxf_summary: None,
    },
    ApiDoc {
        name: "length",
        signature: "(length list) -> integer",
        description: "Returns the total number of elements in list.",
        snippet: "(length '(1 2 3 4))\n",
        category: "List Manipulation",
        dxf_summary: None,
    },
    ApiDoc {
        name: "nth",
        signature: "(nth index list) -> element | nil",
        description: "Returns the element at 0-based index in list.",
        snippet: "(nth 1 '(\"A\" \"B\" \"C\")) ; => \"B\"\n",
        category: "List Manipulation",
        dxf_summary: None,
    },
    ApiDoc {
        name: "last",
        signature: "(last list) -> element",
        description: "Returns the last element of list.",
        snippet: "(last '(1 2 3 4)) ; => 4\n",
        category: "List Manipulation",
        dxf_summary: None,
    },
    ApiDoc {
        name: "assoc",
        signature: "(assoc key alist) -> pair | nil",
        description: "Searches an association list for the first pair whose car matches key. Crucial for reading DXF codes.",
        snippet: "(cdr (assoc 40 (entget (entlast))))\n",
        category: "List Manipulation",
        dxf_summary: None,
    },
    ApiDoc {
        name: "member",
        signature: "(member expr list) -> tail-list | nil",
        description: "Returns the tail of list starting with the first element matching expr, or nil if not found.",
        snippet: "(member 'B '(A B C))\n",
        category: "List Manipulation",
        dxf_summary: None,
    },
    ApiDoc {
        name: "subst",
        signature: "(subst new-elem old-elem list) -> list",
        description: "Returns a copy of list with every occurrence of old-elem replaced by new-elem. Crucial for entmod updates.",
        snippet: "(setq ent (subst '(40 . 50.0) (assoc 40 ent) ent))\n",
        category: "List Manipulation",
        dxf_summary: None,
    },

    // ── 9. Strings & Conversions ────────────────────────────────────────────
    ApiDoc {
        name: "strcat",
        signature: "(strcat [str1 str2 ...]) -> string",
        description: "Concatenates multiple strings into a single string.",
        snippet: "(strcat \"Layer: \" \"WALLS\")\n",
        category: "Strings",
        dxf_summary: None,
    },
    ApiDoc {
        name: "strlen",
        signature: "(strlen [str1 ...]) -> integer",
        description: "Returns the total number of characters in the string(s).",
        snippet: "(strlen \"OpenCAD\") ; => 7\n",
        category: "Strings",
        dxf_summary: None,
    },
    ApiDoc {
        name: "substr",
        signature: "(substr str start [length]) -> string",
        description: "Extracts a substring starting at 1-based index start for optional length characters.",
        snippet: "(substr \"OpenCAD\" 1 4) ; => \"Open\"\n",
        category: "Strings",
        dxf_summary: None,
    },
    ApiDoc {
        name: "strcase",
        signature: "(strcase str [which]) -> string",
        description: "Converts str to uppercase (default), or lowercase if which is non-nil.",
        snippet: "(strcase \"Walls\" T) ; => \"walls\"\n",
        category: "Strings",
        dxf_summary: None,
    },
    ApiDoc {
        name: "itoa",
        signature: "(itoa integer) -> string",
        description: "Converts an integer to a string representation.",
        snippet: "(itoa 42) ; => \"42\"\n",
        category: "Strings",
        dxf_summary: None,
    },
    ApiDoc {
        name: "atoi",
        signature: "(atoi string) -> integer",
        description: "Parses a string into an integer.",
        snippet: "(atoi \"42\") ; => 42\n",
        category: "Strings",
        dxf_summary: None,
    },
    ApiDoc {
        name: "atof",
        signature: "(atof string) -> real",
        description: "Parses a string into a floating-point real number.",
        snippet: "(atof \"3.14159\")\n",
        category: "Strings",
        dxf_summary: None,
    },
    ApiDoc {
        name: "rtos",
        signature: "(rtos num [mode [precision]]) -> string",
        description: "Formats a real number as a string. mode: 1=scientific, 2=decimal (default). precision defaults to 4.",
        snippet: "(rtos 25.1256 2 2) ; => \"25.13\"\n",
        category: "Strings",
        dxf_summary: None,
    },
    ApiDoc {
        name: "angtos",
        signature: "(angtos ang-rad [mode [precision]]) -> string",
        description: "Formats an angle in radians as a string. mode: 0=degrees, 1=deg/min/sec, 2=grads, 3=radians.",
        snippet: "(angtos pi 0 1) ; => \"180.0\"\n",
        category: "Strings",
        dxf_summary: None,
    },
    ApiDoc {
        name: "angtof",
        signature: "(angtof str [mode]) -> real | nil",
        description: "Parses a formatted angle string into radians.",
        snippet: "(angtof \"90\" 0) ; => 1.5707963267948966\n",
        category: "Strings",
        dxf_summary: None,
    },
    ApiDoc {
        name: "distof",
        signature: "(distof str [mode]) -> real | nil",
        description: "Parses a distance string into a real number.",
        snippet: "(distof \"12.5\") ; => 12.5\n",
        category: "Strings",
        dxf_summary: None,
    },
    ApiDoc {
        name: "ascii",
        signature: "(ascii str) -> integer",
        description: "Returns the ASCII integer code of the first character in str.",
        snippet: "(ascii \"A\") ; => 65\n",
        category: "Strings",
        dxf_summary: None,
    },
    ApiDoc {
        name: "chr",
        signature: "(chr int) -> string",
        description: "Converts an ASCII integer code to a single-character string.",
        snippet: "(chr 65) ; => \"A\"\n",
        category: "Strings",
        dxf_summary: None,
    },
    ApiDoc {
        name: "read",
        signature: "(read str) -> atom | list",
        description: "Parses the first Lisp expression from str without evaluating it.",
        snippet: "(read \"(10 20 30)\")\n",
        category: "Strings",
        dxf_summary: None,
    },
    ApiDoc {
        name: "wcmatch",
        signature: "(wcmatch str pattern) -> boolean",
        description: "Performs wild-card pattern matching on str using *, ?, #, ~, and comma-separated patterns.",
        snippet: "(wcmatch \"WALL-10\" \"WALL*\") ; => T\n",
        category: "Strings",
        dxf_summary: None,
    },

    // ── 10. Console, Files & Environment ────────────────────────────────────
    ApiDoc {
        name: "princ",
        signature: "(princ [expr [file-desc]]) -> expr",
        description: "Prints expression to the command line without escaping strings. (princ) suppresses echo.",
        snippet: "(princ \"\\nDone.\")\n(princ)\n",
        category: "Console & Files",
        dxf_summary: None,
    },
    ApiDoc {
        name: "prin1",
        signature: "(prin1 [expr [file-desc]]) -> expr",
        description: "Prints expression to the command line escaping strings with quotes and control characters.",
        snippet: "(prin1 '(10 \"text\" 3.14))\n",
        category: "Console & Files",
        dxf_summary: None,
    },
    ApiDoc {
        name: "print",
        signature: "(print [expr [file-desc]]) -> expr",
        description: "Prints a newline, then prin1 of expr, followed by a trailing space.",
        snippet: "(print val)\n",
        category: "Console & Files",
        dxf_summary: None,
    },
    ApiDoc {
        name: "prompt",
        signature: "(prompt msg-str) -> nil",
        description: "Displays a prompt message on the command line.",
        snippet: "(prompt \"\\nStarting process...\")\n",
        category: "Console & Files",
        dxf_summary: None,
    },
    ApiDoc {
        name: "open",
        signature: "(open filename-str mode-str) -> file-desc | nil",
        description: "Opens a file for reading (\"r\"), writing (\"w\"), or appending (\"a\").",
        snippet: "(setq f (open \"data.txt\" \"w\"))\n(write-line \"Point,10,20\" f)\n(close f)\n",
        category: "Console & Files",
        dxf_summary: None,
    },
    ApiDoc {
        name: "close",
        signature: "(close file-desc) -> nil",
        description: "Closes an open file descriptor.",
        snippet: "(close f)\n",
        category: "Console & Files",
        dxf_summary: None,
    },
    ApiDoc {
        name: "read-line",
        signature: "(read-line [file-desc]) -> string | nil",
        description: "Reads a line of text from file-desc or standard input.",
        snippet: "(setq line (read-line f))\n",
        category: "Console & Files",
        dxf_summary: None,
    },
    ApiDoc {
        name: "write-line",
        signature: "(write-line str [file-desc]) -> string",
        description: "Writes a string followed by a newline to file-desc or standard output.",
        snippet: "(write-line \"Header\" f)\n",
        category: "Console & Files",
        dxf_summary: None,
    },
    ApiDoc {
        name: "findfile",
        signature: "(findfile filename-str) -> path-str | nil",
        description: "Searches the system paths for filename and returns its full path if found.",
        snippet: "(setq path (findfile \"myscript.lsp\"))\n",
        category: "Console & Files",
        dxf_summary: None,
    },
    ApiDoc {
        name: "load",
        signature: "(load filename-str [onfailure]) -> value",
        description: "Loads and evaluates an OpenCAD LISP file (.lsp).",
        snippet: "(load \"myscript\")\n",
        category: "Console & Files",
        dxf_summary: None,
    },
    ApiDoc {
        name: "getenv",
        signature: "(getenv var-name) -> string | nil",
        description: "Retrieves the value of an operating system environment variable.",
        snippet: "(getenv \"APPDATA\")\n",
        category: "Console & Files",
        dxf_summary: None,
    },
    ApiDoc {
        name: "setenv",
        signature: "(setenv var-name value) -> string",
        description: "Sets the value of an operating system environment variable.",
        snippet: "(setenv \"MY_CONFIG\" \"C:/Config\")\n",
        category: "Console & Files",
        dxf_summary: None,
    },

    // ── 11. Extended LISP Functions (vl-*) ───────────────────────────────────
    ApiDoc {
        name: "vl-load-com",
        signature: "(vl-load-com) -> T",
        description: "Initializes extended LISP and object model environment.",
        snippet: "(vl-load-com)\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-string-search",
        signature: "(vl-string-search pattern str [start-pos]) -> integer | nil",
        description: "Returns 0-based index of pattern in str, or nil if not found.",
        snippet: "(vl-string-search \"CAD\" \"OpenCAD\") ; => 4\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-string-subst",
        signature: "(vl-string-subst new-str pattern str [start-pos]) -> string",
        description: "Replaces the first occurrence of pattern in str with new-str.",
        snippet: "(vl-string-subst \"2D\" \"3D\" \"3D Model\") ; => \"2D Model\"\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-string-trim",
        signature: "(vl-string-trim char-set str) -> string",
        description: "Strips characters in char-set from both ends of str.",
        snippet: "(vl-string-trim \" \\t\\n\" \"  hello  \") ; => \"hello\"\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-string-left-trim",
        signature: "(vl-string-left-trim char-set str) -> string",
        description: "Strips characters in char-set from the beginning of str.",
        snippet: "(vl-string-left-trim \" \" \"  abc\") ; => \"abc\"\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-string-right-trim",
        signature: "(vl-string-right-trim char-set str) -> string",
        description: "Strips characters in char-set from the end of str.",
        snippet: "(vl-string-right-trim \" \" \"abc  \") ; => \"abc\"\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-string-translate",
        signature: "(vl-string-translate src-set dest-set str) -> string",
        description: "Replaces characters in str matching src-set with corresponding characters in dest-set.",
        snippet: "(vl-string-translate \"abc\" \"123\" \"cab\") ; => \"312\"\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-string->list",
        signature: "(vl-string->list str) -> integer-list",
        description: "Converts a string into a list of ASCII character codes.",
        snippet: "(vl-string->list \"AB\") ; => (65 66)\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-list->string",
        signature: "(vl-list->string int-list) -> string",
        description: "Converts a list of ASCII integer codes into a string.",
        snippet: "(vl-list->string '(65 66)) ; => \"AB\"\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-remove",
        signature: "(vl-remove elem list) -> list",
        description: "Removes all elements equal to elem from list.",
        snippet: "(vl-remove 0 '(1 0 2 0 3)) ; => (1 2 3)\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-remove-if",
        signature: "(vl-remove-if predicate-fn list) -> list",
        description: "Removes all elements from list for which predicate-fn returns non-nil.",
        snippet: "(vl-remove-if 'minusp '(-1 2 -3 4)) ; => (2 4)\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-remove-if-not",
        signature: "(vl-remove-if-not predicate-fn list) -> list",
        description: "Keeps only elements from list for which predicate-fn returns non-nil (filter).",
        snippet: "(vl-remove-if-not 'numberp '(1 \"A\" 2 \"B\")) ; => (1 2)\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-position",
        signature: "(vl-position elem list) -> integer | nil",
        description: "Returns the 0-based index of elem in list, or nil if not found.",
        snippet: "(vl-position \"B\" '(\"A\" \"B\" \"C\")) ; => 1\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-sort",
        signature: "(vl-sort list compare-fn) -> sorted-list",
        description: "Sorts list using the provided comparison function.",
        snippet: "(vl-sort '(5 2 8 1) '<) ; => (1 2 5 8)\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-sort-i",
        signature: "(vl-sort-i list compare-fn) -> index-list",
        description: "Returns a list of 0-based indices corresponding to the sorted order of elements in list.",
        snippet: "(vl-sort-i '(50 20 80) '<) ; => (1 0 2)\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-every",
        signature: "(vl-every predicate-fn list1 [list2 ...]) -> boolean",
        description: "Returns T if predicate-fn returns non-nil for every element in list(s).",
        snippet: "(vl-every 'numberp '(1 2 3))\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-some",
        signature: "(vl-some predicate-fn list1 [list2 ...]) -> value",
        description: "Returns the first non-nil result of predicate-fn applied to elements of list(s).",
        snippet: "(vl-some 'minusp '(1 -2 3)) ; => T\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-filename-base",
        signature: "(vl-filename-base filepath-str) -> string",
        description: "Extracts the base file name from a path without directory or extension.",
        snippet: "(vl-filename-base \"C:/drawings/part.dwg\") ; => \"part\"\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-filename-directory",
        signature: "(vl-filename-directory filepath-str) -> string | nil",
        description: "Extracts the directory portion of a file path.",
        snippet: "(vl-filename-directory \"C:/drawings/part.dwg\") ; => \"C:/drawings\"\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-filename-extension",
        signature: "(vl-filename-extension filepath-str) -> string | nil",
        description: "Extracts the extension from a file path including the leading dot (e.g. \".dwg\").",
        snippet: "(vl-filename-extension \"part.dwg\") ; => \".dwg\"\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-file-copy",
        signature: "(vl-file-copy src-path dst-path [append-flag]) -> integer | nil",
        description: "Copies or appends a file on disk.",
        snippet: "(vl-file-copy \"src.txt\" \"dst.txt\")\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-file-delete",
        signature: "(vl-file-delete filepath-str) -> boolean",
        description: "Deletes a file from the disk.",
        snippet: "(vl-file-delete \"temp.txt\")\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-file-size",
        signature: "(vl-file-size filepath-str) -> integer | nil",
        description: "Returns the size of a file in bytes, or nil if file not found.",
        snippet: "(vl-file-size \"part.dxf\")\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-directory-files",
        signature: "(vl-directory-files [dir [pattern [mode]]]) -> string-list",
        description: "Lists files or folders in dir. mode: -1=dirs only, 1=files only, 0=all.",
        snippet: "(vl-directory-files \".\" \"*.lsp\" 1)\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-catch-all-apply",
        signature: "(vl-catch-all-apply func arg-list) -> value | error-object",
        description: "Safely invokes func with args catching any execution error without aborting.",
        snippet: "(setq res (vl-catch-all-apply '/ '(10 0)))\n(if (vl-catch-all-error-p res)\n  (princ (vl-catch-all-error-message res))\n)\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-catch-all-error-p",
        signature: "(vl-catch-all-error-p obj) -> boolean",
        description: "Returns T if obj is an error object returned by vl-catch-all-apply, nil otherwise.",
        snippet: "(vl-catch-all-error-p res)\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vl-catch-all-error-message",
        signature: "(vl-catch-all-error-message error-obj) -> string",
        description: "Extracts the error message string from an error object returned by vl-catch-all-apply.",
        snippet: "(princ (vl-catch-all-error-message res))\n",
        category: "LISP Extensions",
        dxf_summary: None,
    },

    // ── 12. Object Model Emulation (vlax-* / vla-*) ─────────────────────────
    ApiDoc {
        name: "vlax-get-acad-object",
        signature: "(vlax-get-acad-object) -> vla-object",
        description: "Returns the top-level CAD application VLA object.",
        snippet: "(setq app (vlax-get-acad-object))\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vla-get-ActiveDocument",
        signature: "(vla-get-ActiveDocument app) -> vla-object",
        description: "Returns the active drawing document VLA object for app.",
        snippet: "(setq doc (vla-get-ActiveDocument (vlax-get-acad-object)))\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vla-StartUndoMark",
        signature: "(vla-StartUndoMark doc) -> nil",
        description: "Begins a grouped undo mark on doc so subsequent drawing edits undo as a single step until vla-EndUndoMark is called.",
        snippet: "(vla-StartUndoMark doc)\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vla-EndUndoMark",
        signature: "(vla-EndUndoMark doc) -> nil",
        description: "Ends the current grouped undo mark started by vla-StartUndoMark on doc.",
        snippet: "(vla-EndUndoMark doc)\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-ename->vla-object",
        signature: "(vlax-ename->vla-object ename) -> vla-object",
        description: "Converts an OpenCAD LISP entity name (ENAME) to a VLA object wrapper.",
        snippet: "(setq obj (vlax-ename->vla-object (entlast)))\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-vla-object->ename",
        signature: "(vlax-vla-object->ename vla-obj) -> ename",
        description: "Converts a VLA object wrapper back to an OpenCAD LISP entity name (ENAME).",
        snippet: "(setq ent (vlax-vla-object->ename obj))\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-get-property",
        signature: "(vlax-get-property vla-obj prop-sym) -> value",
        description: "Retrieves an object property (e.g. 'Layer, 'Radius, 'Center, 'Area, 'Length) from vla-obj.",
        snippet: "(setq r (vlax-get-property obj 'Radius))\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-put-property",
        signature: "(vlax-put-property vla-obj prop-sym value) -> nil",
        description: "Sets an object property (e.g. 'Layer, 'Radius, 'Color) on vla-obj.",
        snippet: "(vlax-put-property obj 'Layer \"WALLS\")\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-get",
        signature: "(vlax-get vla-obj prop-sym) -> value",
        description: "Shorthand alias for vlax-get-property with automatic type conversion.",
        snippet: "(setq r (vlax-get obj 'Radius))\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-put",
        signature: "(vlax-put vla-obj prop-sym value) -> nil",
        description: "Shorthand alias for vlax-put-property with automatic type conversion.",
        snippet: "(vlax-put obj 'Layer \"0\")\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-invoke-method",
        signature: "(vlax-invoke-method vla-obj method-sym [args ...]) -> value",
        description: "Invokes an object method (e.g. 'Delete, 'Update) on vla-obj.",
        snippet: "(vlax-invoke-method obj 'Delete)\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-invoke",
        signature: "(vlax-invoke vla-obj method-sym [args ...]) -> value",
        description: "Shorthand alias for vlax-invoke-method with automatic argument conversion.",
        snippet: "(vlax-invoke obj 'Update)\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-ldata-get",
        signature: "(vlax-ldata-get dict-key item-key [default-val]) -> value",
        description: "Retrieves Lisp data stored under dict-key and item-key in the drawing's extension dictionary.",
        snippet: "(setq val (vlax-ldata-get \"MyApp\" \"Scale\" 1.0))\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-ldata-put",
        signature: "(vlax-ldata-put dict-key item-key value) -> value",
        description: "Stores arbitrary Lisp data persistently in the drawing under dict-key and item-key.",
        snippet: "(vlax-ldata-put \"MyApp\" \"Scale\" 2.5)\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-ldata-delete",
        signature: "(vlax-ldata-delete dict-key item-key) -> boolean",
        description: "Deletes an LData item from the drawing dictionary.",
        snippet: "(vlax-ldata-delete \"MyApp\" \"Scale\")\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-ldata-list",
        signature: "(vlax-ldata-list dict-key) -> alist",
        description: "Lists all (key . value) pairs stored under dict-key.",
        snippet: "(vlax-ldata-list \"MyApp\")\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-curve-getstartpoint",
        signature: "(vlax-curve-getstartpoint curve-ent) -> (x y z)",
        description: "Returns the 3D start point of a curve entity (Line, Arc, Polyline, Circle).",
        snippet: "(setq p1 (vlax-curve-getstartpoint (entlast)))\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-curve-getendpoint",
        signature: "(vlax-curve-getendpoint curve-ent) -> (x y z)",
        description: "Returns the 3D end point of a curve entity (Line, Arc, Polyline, Circle).",
        snippet: "(setq p2 (vlax-curve-getendpoint (entlast)))\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-make-variant",
        signature: "(vlax-make-variant [value [type-int]]) -> variant",
        description: "Wraps a Lisp value into a variant container.",
        snippet: "(setq var (vlax-make-variant 100.0))\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-variant-value",
        signature: "(vlax-variant-value variant) -> value",
        description: "Unwraps and returns the raw Lisp value from a variant.",
        snippet: "(vlax-variant-value var)\n",
        category: "Object Model",
        dxf_summary: None,
    },
    ApiDoc {
        name: "vlax-make-safearray",
        signature: "(vlax-make-safearray type-int bounds-list) -> safearray",
        description: "Creates a SafeArray container.",
        snippet: "(vlax-make-safearray 5 '(0 . 2))\n",
        category: "Object Model",
        dxf_summary: None,
    },

    // ── 13. OpenCAD Native UI Dialogs & Panels (oc-*) ──────────────────────
    ApiDoc {
        name: "oc-dialog",
        signature: "(oc-dialog title [options] row1 [row2 ...]) -> dialog-def",
        description: "Constructs a modal dialog definition with interactive controls. Parameters:
- title (string): Dialog window title
- [options] (optional alist): '((width . 400.0) (submit . \"OK\") (cancel . \"Cancel\"))
- row1 [row2 ...] (controls): One or more rows, columns, or controls",
        snippet: "(setq dlg (oc-dialog \"Parameters\"
  '((width . 400.0))
  (oc-row (oc-number \"rad\" \"Radius\" 25.0 '((min . 1.0) (max . 100.0))))
  (oc-row (oc-checkbox \"draw_x\" \"Draw X\" nil))
))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-dialog-show",
        signature: "(oc-dialog-show dialog-def [action_callback]) -> alist | nil",
        description: "Displays a dialog modally and returns an alist of (id . value) pairs if submitted, or nil on Cancel. Parameters:
- dialog-def: Dialog object from oc-dialog
- [action_callback] (optional symbol or lambda): (callback control-id action current-values) for two-way reactive interactivity without closing",
        snippet: "(setq res (oc-dialog-show dlg))
(if res
  (princ (oc-get-real \"rad\" res))
)
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-get",
        signature: "(oc-get key-str alist) -> value",
        description: "Retrieves raw control value by key from the result alist returned by oc-dialog-show. Parameters:
- key-str (string | symbol): Control ID key
- alist: Association list returned by oc-dialog-show",
        snippet: "(setq v (oc-get \"rad\" res))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-get-real",
        signature: "(oc-get-real key-str alist) -> real",
        description: "Retrieves a numeric floating-point value by key from dialog results. Parameters:
- key-str (string | symbol): Control ID key
- alist: Association list returned by oc-dialog-show",
        snippet: "(setq r (oc-get-real \"rad\" res))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-get-str",
        signature: "(oc-get-str key-str alist) -> string",
        description: "Retrieves a text string value by key from dialog results. Parameters:
- key-str (string | symbol): Control ID key
- alist: Association list returned by oc-dialog-show",
        snippet: "(setq s (oc-get-str \"label\" res))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-get-bool",
        signature: "(oc-get-bool key-str alist) -> boolean",
        description: "Retrieves a boolean flag (T or nil) by key from dialog results. Parameters:
- key-str (string | symbol): Control ID key
- alist: Association list returned by oc-dialog-show",
        snippet: "(setq b (oc-get-bool \"draw_x\" res))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-get-int",
        signature: "(oc-get-int key-str alist) -> integer",
        description: "Retrieves an integer value by key from dialog results. Parameters:
- key-str (string | symbol): Control ID key
- alist: Association list returned by oc-dialog-show",
        snippet: "(setq n (oc-get-int \"count\" res))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-panel-register",
        signature: "(oc-panel-register panel-id title [options] ctrl1 [ctrl2 ...]) -> boolean",
        description: "Registers a persistent dockable side panel in OpenCAD Studio. Parameters:
- panel-id (string | symbol): Unique panel ID
- title (string | symbol): Header title on panel tab
- [options] (optional alist): '((width . 260.0) (command . \"MYCMD\") (style . \"palette\")) — style defaults to \"dialog\" / \"form\" (dialog-like form style); pass \"palette\" for the compact Properties-grid style
- ctrl1 [ctrl2 ...] (controls): Controls placed vertically in panel",
        snippet: "(oc-panel-register \"my_panel\" \"Tools\"
  '((width . 260.0))
  (oc-choice \"col\" \"Color\" '(\"Red\" \"Blue\") \"Red\")
  (oc-button \"btn\" \"Execute\" \"(c:run)\")
)
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-panel-toggle",
        signature: "(oc-panel-toggle panel-id [show-bool]) -> boolean",
        description: "Toggles or sets visibility of a registered dockable side panel. Parameters:
- panel-id (string | symbol): Identifier of registered panel
- [show-bool] (optional boolean): T to show, nil to hide; toggles if omitted",
        snippet: "(oc-panel-toggle \"my_panel\" T)
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-panel-update",
        signature: "(oc-panel-update panel-id values-alist) -> boolean",
        description: "Updates control values in a live dockable side panel without re-registering. Parameters:
- panel-id (string | symbol): Identifier of registered panel
- values-alist: Alist of ((\"control_id\" . new-value) ...)",
        snippet: "(oc-panel-update \"my_panel\" (list (cons \"col\" \"Blue\")))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-panel-get",
        signature: "(oc-panel-get panel-id control-id) -> value",
        description: "Reads the current live value of a control inside a dockable side panel. Parameters:
- panel-id (string | symbol): Side panel ID
- control-id (string | symbol): Control ID inside panel",
        snippet: "(setq val (oc-panel-get \"my_panel\" \"col\"))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-panel-get-num",
        signature: "(oc-panel-get-num panel-id control-id) -> real",
        description: "Reads the current numeric value of a control inside a dockable side panel. Parameters:
- panel-id (string | symbol): Side panel ID
- control-id (string | symbol): Numeric control ID inside panel",
        snippet: "(setq n (oc-panel-get-num \"my_panel\" \"radius\"))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-panel-get-real",
        signature: "(oc-panel-get-real panel-id control-id) -> real",
        description: "Reads the current real numeric value of a control inside a dockable side panel (alias of oc-panel-get-num). Parameters:
- panel-id (string | symbol): Side panel ID
- control-id (string | symbol): Numeric control ID inside panel",
        snippet: "(setq r (oc-panel-get-real \"my_panel\" \"radius\"))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-panel-get-str",
        signature: "(oc-panel-get-str panel-id control-id) -> string",
        description: "Reads the current string or selected choice text of a control inside a dockable side panel. Parameters:
- panel-id (string | symbol): Side panel ID
- control-id (string | symbol): Control ID inside panel",
        snippet: "(setq s (oc-panel-get-str \"my_panel\" \"color\"))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-panel-get-bool",
        signature: "(oc-panel-get-bool panel-id control-id) -> boolean",
        description: "Reads the current boolean checkbox state (T or nil) of a control inside a dockable side panel. Parameters:
- panel-id (string | symbol): Side panel ID
- control-id (string | symbol): Checkbox control ID inside panel",
        snippet: "(if (oc-panel-get-bool \"my_panel\" \"chk_close\") (princ \"Closed\"))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-row",
        signature: "(oc-row [options] ctrl1 [ctrl2 ...]) -> row-def",
        description: "Groups multiple controls horizontally in a single row within a dialog. Parameters:
- [options] (optional alist): '((align-h . \"left\"|\"center\"|\"right\") (align-v . \"top\"|\"middle\"|\"bottom\"))
- ctrl1 [ctrl2 ...] (controls): Child controls in row",
        snippet: "(oc-row '((align-h . \"center\") (align-v . \"middle\"))
  (oc-number \"w\" \"Width\" 10.0)
  (oc-number \"h\" \"Height\" 20.0)
)
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-column",
        signature: "(oc-column [options] ctrl1 [ctrl2 ...]) -> control-def",
        description: "Groups multiple controls vertically in a single column container. Parameters:
- [options] (optional alist): '((align-h . \"left\"|\"center\"|\"right\") (align-v . \"top\"|\"middle\"|\"bottom\"))
- ctrl1 [ctrl2 ...] (controls): Stacked child controls",
        snippet: "(oc-column '((align-h . \"center\") (align-v . \"top\"))
  (oc-text \"txt1\" \"Label\" \"Val\")
  (oc-button \"btn1\" \"Submit\")
)
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-text",
        signature: "(oc-text id-str label-str [default-str]) -> control-def",
        description: "UI Control: Creates a single-line text input field. Parameters:
- id-str (string | symbol): Control ID
- label-str (string | symbol): Field label
- [default-str] (optional string, default \"\"): Initial text value",
        snippet: "(oc-text \"txt_name\" \"Name\" \"Part-1\")
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-number",
        signature: "(oc-number id-str label-str [default-num] [options]) -> control-def",
        description: "UI Control: Creates a numeric input field with spinner constraints. Parameters:
- id-str (string | symbol): Control ID
- label-str (string | symbol): Field label
- [default-num] (optional number or alist, default 0.0): Initial value or options alist
- [options] (optional alist): '((min . 0.0) (max . 100.0) (step . 1.0) (default . 0.0))",
        snippet: "(oc-number \"rad\" \"Radius\" 25.0 '((min . 1.0) (max . 100.0) (step . 0.5)))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-checkbox",
        signature: "(oc-checkbox id-str label-str [default-bool] [options]) -> control-def",
        description: "UI Control: Creates a boolean checkbox toggle. Parameters:
- id-str (string | symbol): Control ID
- label-str (string | symbol): Field label
- [default-bool] (optional boolean, default nil): Initial state (T or nil)
- [options] (optional alist): '((align-h . \"left\"|\"center\"|\"right\") (align-v . \"top\"|\"middle\"|\"bottom\"))",
        snippet: "(oc-checkbox \"chk_close\" \"Close Shape\" T)
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-choice",
        signature: "(oc-choice id-str label-str options-list [default-choice]) -> control-def",
        description: "UI Control: Creates a dropdown selector menu. Parameters:
- id-str (string | symbol): Control ID
- label-str (string | symbol): Field label
- options-list (list of strings): Dropdown items list
- [default-choice] (optional int, string, or alist, default 0): 0-based index, string match, or '((default . 0))",
        snippet: "(oc-choice \"cmb_layer\" \"Layer\" '(\"0\" \"WALLS\" \"DIMS\") \"0\")
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-color",
        signature: "(oc-color id-str label-str [default-rgb]) -> control-def",
        description: "UI Control: Creates an RGB color picker. Parameters:
- id-str (string | symbol): Control ID
- label-str (string | symbol): Field label
- [default-rgb] (optional list of 3 ints, default '(0 0 0)): RGB values (0..255)",
        snippet: "(oc-color \"clr\" \"Color\" '(255 0 0))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-file",
        signature: "(oc-file id-str label-str [default-path] [options]) -> control-def",
        description: "UI Control: Creates a native file open/save browser control. Parameters:
- id-str (string | symbol): Control ID
- label-str (string | symbol): Field label
- [default-path] (optional string, default \"\"): Initial path
- [options] (optional alist): '((filter . \"DXF Files (*.dxf)\") (extensions . (\"dxf\")) (save . T|nil))",
        snippet: "(oc-file \"out_file\" \"DXF File\" \"output.dxf\" '((filter . \"DXF Files (*.dxf)\") (extensions . (\"dxf\")) (save . T)))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-table",
        signature: "(oc-table id-str label-str headers-list [rows-list] [options]) -> control-def",
        description: "UI Control: Creates an interactive data table grid with row add/remove buttons. Parameters:
- id-str (string | symbol): Control ID
- label-str (string | symbol): Table title
- headers-list (list of strings): Column header titles
- [rows-list] (optional list of row lists): Initial 2D table data
- [options] (optional alist): '((allow_add . T|nil) (allow_delete . T|nil))",
        snippet: "(oc-table \"tbl_pts\" \"Coordinates\" '(\"X\" \"Y\" \"Z\") '((\"0.0\" \"0.0\" \"0.0\")) '((allow_add . T) (allow_delete . T)))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-fields",
        signature: "(oc-fields [id-str] title-str [options] ctrl1 [ctrl2 ...]) -> control-def",
        description: "UI Control: Creates a collapsible property group box. Parameters:
- [id-str] (optional string | symbol): Section ID
- title-str (string | symbol): Header title on group box
- [options] (optional alist): '((collapsed . T|nil))
- ctrl1 [ctrl2 ...] (controls): Nested controls",
        snippet: "(oc-fields \"Geometry\"
  '((collapsed . nil))
  (oc-number \"len\" \"Length\" 50.0)
  (oc-number \"wid\" \"Width\" 30.0)
)
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-button",
        signature: "(oc-button id-str label-str [action] [options]) -> control-def",
        description: "UI Control: Creates a clickable button. Parameters:
- id-str (string | symbol): Control ID
- label-str (string | symbol): Button text
- [action] (optional string or alist, default \"click\"): Command string in panels; action ID in dialogs
- [options] (optional alist): '((action . \"...\") (align-h . \"left\"|\"center\"|\"right\") (width . 140.0))",
        snippet: "(oc-button \"btn_draw\" \"Draw Shape\" \"(c:draw-shape)\" '((align-h . \"center\") (width . 140.0)))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-info",
        signature: "(oc-info [id-str] text-str [options]) -> control-def",
        description: "UI Control: Displays read-only instructional or descriptive text. Parameters:
- [id-str] (optional string | symbol): Optional control ID for live updates
- text-str (string | symbol): Label text
- [options] (optional alist): '((align-h . \"left\"|\"center\"|\"right\"))",
        snippet: "(oc-info \"Specify dimensions in millimeters below:\" '((align-h . \"center\")))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "msgbox",
        signature: "(msgbox prompt-str [title-str] [buttons-spec]) -> button-id",
        description: "Displays a native message box dialog. Parameters:
- prompt-str (string | any): Dialog message text
- [title-str] (optional string, default \"OpenCAD Studio\"): Title bar text
- [buttons-spec] (optional int, symbol, or alist): \"OK\", \"OKCANCEL\", \"YESNO\", \"RETRYCANCEL\", \"ABORTRETRYIGNORE\", \"YESNOCANCEL\". Returns: 1=OK, 2=Cancel, 3=Abort, 4=Retry, 5=Ignore, 6=Yes, 7=No",
        snippet: "(if (= 6 (msgbox \"Delete entity?\" \"Confirm\" \"YESNO\")) (entdel ent))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
    ApiDoc {
        name: "oc-msgbox",
        signature: "(oc-msgbox prompt-str [title-str] [buttons-spec]) -> button-id",
        description: "Displays a native message box dialog. Parameters:
- prompt-str (string | any): Dialog message text
- [title-str] (optional string, default \"OpenCAD Studio\"): Title bar text
- [buttons-spec] (optional int, symbol, or alist): \"OK\", \"OKCANCEL\", \"YESNO\", \"RETRYCANCEL\", \"ABORTRETRYIGNORE\", \"YESNOCANCEL\", or alist '((buttons . yesno) (title . \"...\") (icon . warning)). Returns: 1=OK, 2=Cancel, 3=Abort, 4=Retry, 5=Ignore, 6=Yes, 7=No",
        snippet: "(if (= 6 (oc-msgbox \"Proceed?\" \"Notice\" \"YESNO\")) (princ \"Confirmed\"))
",
        category: "UI & Panels",
        dxf_summary: None,
    },
];

/// Tests whether a character is valid in an OpenCAD LISP symbol identifier.
pub fn is_lisp_ident_char(c: char) -> bool {
    c.is_alphanumeric()
        || c == '-'
        || c == '_'
        || c == '*'
        || c == '+'
        || c == '/'
        || c == '='
        || c == '<'
        || c == '>'
        || c == ':'
        || c == '~'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_docs_completeness_and_categories() {
        assert!(API_DOCS.len() >= 90, "Expected at least 90 OpenCAD LISP functions documented, got {}", API_DOCS.len());

        let categories: std::collections::HashSet<&str> = API_DOCS.iter().map(|d| d.category).collect();
        assert!(categories.contains("CAD Database"));
        assert!(categories.contains("CAD Commands"));
        assert!(categories.contains("Interactive"));
        assert!(categories.contains("Selection Sets"));
        assert!(categories.contains("Control Flow"));
        assert!(categories.contains("Logic & Predicates"));
        assert!(categories.contains("Math & Geometry"));
        assert!(categories.contains("List Manipulation"));
        assert!(categories.contains("Strings"));
        assert!(categories.contains("Console & Files"));
        assert!(categories.contains("LISP Extensions"));
        assert!(categories.contains("Object Model"));
        assert!(categories.contains("UI & Panels"));
    }

    #[test]
    fn test_entmake_and_entmod_dxf_summaries() {
        let entmake_doc = API_DOCS.iter().find(|d| d.name == "entmake").expect("entmake doc");
        assert!(entmake_doc.dxf_summary.is_some());
        let em_dxf = entmake_doc.dxf_summary.unwrap();
        assert!(em_dxf.contains("LINE:       (0 . \"LINE\") (10 x y z) (11 x y z)"));
        assert!(em_dxf.contains("CIRCLE:     (0 . \"CIRCLE\") (10 x y z) (40 . radius)"));
        assert!(em_dxf.contains("ARC:        (0 . \"ARC\") (10 x y z) (40 . radius) (50 . start_deg) (51 . end_deg)"));
        assert!(em_dxf.contains("LWPOLYLINE: (0 . \"LWPOLYLINE\") (70 . flag) (90 . n)"));
        assert!(em_dxf.contains("TEXT:       (0 . \"TEXT\") (1 . \"str\") (10 x y z) (40 . height)"));
        assert!(em_dxf.contains("MTEXT:      (0 . \"MTEXT\") (1 . \"str\") (10 x y z) (40 . height)"));
        assert!(em_dxf.contains("POINT:      (0 . \"POINT\") (10 x y z)"));
        assert!(em_dxf.contains("SOLID:      (0 . \"SOLID\") (10 p1) (11 p2) (12 p3) (13 p4)"));
        assert!(em_dxf.contains("INSERT:     (0 . \"INSERT\") (2 . \"BLOCK\") (10 x y z)"));
        assert!(em_dxf.contains("ELLIPSE:    (0 . \"ELLIPSE\") (10 center) (11 major_axis) (40 . ratio)"));
        assert!(em_dxf.contains("-3 XDATA:   (-3 (\"APPID\" (1000 . \"str\") (1040 . real) (1070 . int) (1010 x y z)))"));

        let entmod_doc = API_DOCS.iter().find(|d| d.name == "entmod").expect("entmod doc");
        assert!(entmod_doc.dxf_summary.is_some());
        let emod_dxf = entmod_doc.dxf_summary.unwrap();
        assert!(emod_dxf.contains("(5 . \"HANDLE\")"));
        assert!(emod_dxf.contains("(0 . \"TYPE\")"));
        assert!(emod_dxf.contains("-3 (\"APPID\""));
    }

    #[test]
    fn test_is_lisp_ident_char() {
        assert!(is_lisp_ident_char('a'));
        assert!(is_lisp_ident_char('Z'));
        assert!(is_lisp_ident_char('0'));
        assert!(is_lisp_ident_char('-'));
        assert!(is_lisp_ident_char('_'));
        assert!(is_lisp_ident_char('*'));
        assert!(is_lisp_ident_char('+'));
        assert!(is_lisp_ident_char('/'));
        assert!(is_lisp_ident_char(':'));
        assert!(is_lisp_ident_char('~'));

        // Delimiters
        assert!(!is_lisp_ident_char('.'));
        assert!(!is_lisp_ident_char('('));
        assert!(!is_lisp_ident_char(')'));
        assert!(!is_lisp_ident_char(' '));
        assert!(!is_lisp_ident_char('"'));
        assert!(!is_lisp_ident_char('\''));
    }
}
