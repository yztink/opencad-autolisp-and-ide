# ============================================================================
# DO NOT EDIT — LEGACY PYTHON OPENCAD LISP PROTOTYPE (SHELVED / OBSOLETE).
# This file is NOT used for OpenCAD LISP execution.
# All OpenCAD LISP execution goes straight to the pure-Rust opencad-lisp engine:
#   - ../opencad-lisp-engine (crates/lisp-core, crates/lisp-cad)
#   - src/rust_lisp.rs
# ============================================================================
"""
OpenCAD LISP Engine (opencad_lisp.py) — LEGACY / SHELVED
"""

import sys
import re
import math
import inspect
from typing import Any, List, Tuple, Dict, Optional, Callable


# ─────────────────────────────────────────────────────────────────────────────
# 1. Fundamental Types & Symbols
# ─────────────────────────────────────────────────────────────────────────────

class Symbol:
    """Case-insensitive OpenCAD LISP symbol."""
    __slots__ = ("name",)

    def __init__(self, name: str):
        self.name = name.upper()

    def __repr__(self) -> str:
        return self.name

    def __hash__(self) -> int:
        return hash(self.name)

    def __eq__(self, other: Any) -> bool:
        if isinstance(other, Symbol):
            return self.name == other.name
        if isinstance(other, str):
            return self.name == other.upper()
        return False


class Cons:
    """OpenCAD LISP cons cell (car . cdr) used for lists and dotted pairs (e.g. DXF)."""
    __slots__ = ("car", "cdr")

    def __init__(self, car: Any, cdr: Any = None):
        self.car = car
        self.cdr = cdr

    def __repr__(self) -> str:
        # Check if proper list
        items = []
        curr = self
        while isinstance(curr, Cons):
            items.append(lisp_repr(curr.car))
            curr = curr.cdr
        if curr is None or curr is NIL:
            return "(" + " ".join(items) + ")"
        else:
            return "(" + " ".join(items) + " . " + lisp_repr(curr) + ")"

    def to_list(self) -> List[Any]:
        """Convert a proper cons list to a Python list."""
        out = []
        curr = self
        while isinstance(curr, Cons):
            out.append(curr.car)
            curr = curr.cdr
        if curr is not None and curr is not NIL:
            out.append(curr)
        return out


class LispNil:
    """OpenCAD LISP nil singleton representing falsity and the empty list."""
    def __repr__(self) -> str:
        return "nil"

    def __bool__(self) -> bool:
        return False

    def __eq__(self, other: Any) -> bool:
        return other is None or isinstance(other, LispNil) or other == [] or other == ()


class LispT:
    """OpenCAD LISP T singleton representing truth."""
    def __repr__(self) -> str:
        return "T"

    def __bool__(self) -> bool:
        return True

    def __eq__(self, other: Any) -> bool:
        return other is True or isinstance(other, LispT)


NIL = LispNil()
T = LispT()


def lisp_repr(val: Any) -> str:
    """Format a Python/Lisp value into OpenCAD LISP representation."""
    if val is None or val is NIL:
        return "nil"
    if val is True or val is T:
        return "T"
    if val is False:
        return "nil"
    if isinstance(val, str):
        return f'"{val}"'
    if isinstance(val, float):
        # Format cleanly
        if val.is_integer():
            return f"{val:.1f}"
        return f"{val:.6g}"
    if isinstance(val, (list, tuple)):
        return "(" + " ".join(lisp_repr(x) for x in val) + ")"
    if isinstance(val, Cons):
        return repr(val)
    if isinstance(val, Symbol):
        return val.name
    return repr(val)


# ─────────────────────────────────────────────────────────────────────────────
# 2. Tokenizer & Parser (Reader)
# ─────────────────────────────────────────────────────────────────────────────

def tokenize(source: str) -> List[str]:
    """Tokenize OpenCAD LISP source code into syntactic tokens."""
    tokens = []
    i = 0
    n = len(source)

    while i < n:
        c = source[i]

        # Whitespace
        if c.isspace():
            i += 1
            continue

        # Line comment: semicolon to end of line
        if c == ';':
            while i < n and source[i] != '\n':
                i += 1
            continue

        # Parentheses, Quote, Dotted Dot
        if c in '()\'`':
            tokens.append(c)
            i += 1
            continue

        if c == ',':
            if i + 1 < n and source[i + 1] == '@':
                tokens.append(',@')
                i += 2
            else:
                tokens.append(',')
                i += 1
            continue

        # String literal
        if c == '"':
            start = i
            i += 1
            s_buf = []
            while i < n and source[i] != '"':
                if source[i] == '\\' and i + 1 < n:
                    i += 1
                    esc = source[i]
                    if esc == 'n': s_buf.append('\n')
                    elif esc == 't': s_buf.append('\t')
                    elif esc == 'r': s_buf.append('\r')
                    elif esc == '"': s_buf.append('"')
                    elif esc == '\\': s_buf.append('\\')
                    else: s_buf.append(esc)
                else:
                    s_buf.append(source[i])
                i += 1
            if i < n:
                i += 1  # consume closing quote
            tokens.append('"' + "".join(s_buf) + '"')
            continue

        # Atom (Symbol, Number, Dotted pair dot)
        start = i
        while i < n and not source[i].isspace() and source[i] not in '()";\'`':
            i += 1
        atom = source[start:i]
        tokens.append(atom)

    return tokens


def parse_atom(token: str) -> Any:
    """Parse a single token into a literal number, string, boolean, or Symbol."""
    if token.startswith('"') and token.endswith('"'):
        return token[1:-1]
    
    # Try parsing int
    try:
        return int(token)
    except ValueError:
        pass

    # Try parsing float
    try:
        return float(token)
    except ValueError:
        pass

    # Booleans & Nil
    upper = token.upper()
    if upper == "NIL":
        return NIL
    if upper == "T":
        return T

    return Symbol(upper)


def read_from_tokens(tokens: List[str]) -> Any:
    """Read a single S-expression from the token stream."""
    if not tokens:
        raise SyntaxError("Unexpected EOF while parsing OpenCAD LISP")

    token = tokens.pop(0)

    # Shorthand quote: 'expr -> (quote expr)
    if token == "'":
        sub = read_from_tokens(tokens)
        return [Symbol("QUOTE"), sub]

    if token == "`":
        sub = read_from_tokens(tokens)
        return [Symbol("QUASIQUOTE"), sub]

    if token == ",":
        sub = read_from_tokens(tokens)
        return [Symbol("UNQUOTE"), sub]

    if token == ",@":
        sub = read_from_tokens(tokens)
        return [Symbol("UNQUOTE-SPLICING"), sub]

    if token == "(":
        elements = []
        is_dotted = False
        dotted_cdr = None

        while tokens and tokens[0] != ")":
            if tokens[0] == ".":
                tokens.pop(0)  # consume '.'
                is_dotted = True
                dotted_cdr = read_from_tokens(tokens)
                if not tokens or tokens[0] != ")":
                    raise SyntaxError("Expected ')' after cdr in dotted pair")
                break
            elements.append(read_from_tokens(tokens))

        if not tokens:
            raise SyntaxError("Missing ')'")
        tokens.pop(0)  # consume ')'

        if is_dotted:
            # Build cons chain ending in dotted_cdr
            if not elements:
                raise SyntaxError("Cannot create dotted pair with no car")
            res = dotted_cdr
            for item in reversed(elements):
                res = Cons(item, res)
            return res

        return elements

    if token == ")":
        raise SyntaxError("Unexpected ')'")

    return parse_atom(token)


def parse_all(source: str) -> List[Any]:
    """Parse all top-level expressions in a source string."""
    tokens = tokenize(source)
    exprs = []
    while tokens:
        exprs.append(read_from_tokens(tokens))
    return exprs


# ─────────────────────────────────────────────────────────────────────────────
# 3. Environment & Evaluation
# ─────────────────────────────────────────────────────────────────────────────

class Env:
    """Lexically scoped environment with case-insensitive symbol lookup."""
    def __init__(self, params: Tuple[Any, ...] = (), args: Tuple[Any, ...] = (), outer: Optional['Env'] = None):
        self.vars: Dict[str, Any] = {}
        self.outer = outer
        for p, a in zip(params, args):
            sym_name = p.name if isinstance(p, Symbol) else str(p).upper()
            self.vars[sym_name] = a

    def find(self, var: Any) -> 'Env':
        """Find the innermost environment where symbol var is bound."""
        name = var.name if isinstance(var, Symbol) else str(var).upper()
        if name in self.vars:
            return self
        if self.outer is not None:
            return self.outer.find(var)
        return self  # Fallback to this environment for new global bindings

    def get(self, var: Any, default: Any = NIL) -> Any:
        name = var.name if isinstance(var, Symbol) else str(var).upper()
        if name in self.vars:
            return self.vars[name]
        if self.outer is not None:
            return self.outer.get(var, default)
        return default

    def set(self, var: Any, val: Any):
        name = var.name if isinstance(var, Symbol) else str(var).upper()
        self.vars[name] = val


class LispFunction:
    """User-defined OpenCAD LISP function closure created by DEFUN or LAMBDA."""
    def __init__(self, name: str, params: List[Symbol], locals_: List[Symbol], body: List[Any], env: Env):
        self.name = name
        self.__name__ = name
        self.__doc__ = f"OpenCAD LISP function {name}"
        self.params = params
        self.locals = locals_
        self.body = body
        self.env = env

    def __call__(self, *args) -> Any:
        if len(args) == 1 and isinstance(args[0], list) and len(self.params) == 0:
            args = ()
        # Bind params to passed args, and locals to NIL
        bound_params = list(self.params) + list(self.locals)
        bound_args = list(args) + [NIL] * max(0, len(bound_params) - len(args))
        call_env = Env(tuple(bound_params), tuple(bound_args), outer=self.env)
        
        result = NIL
        for expr in self.body:
            result = lisp_eval(expr, call_env)
        return result

    def __repr__(self) -> str:
        return f"<OpenCAD LISP-Function {self.name}>"


def lisp_eval(x: Any, env: Env) -> Any:
    """Core recursive S-expression evaluator."""
    while True:
        # 1. Symbol variable reference
        if isinstance(x, Symbol):
            return env.get(x)

        # 2. Literal constant (int, float, str, bool, Nil, Cons)
        if not isinstance(x, list):
            return x

        # 3. Empty list is NIL
        if len(x) == 0:
            return NIL

        op = x[0]

        # ── SPECIAL FORMS ─────────────────────────────────────────────────────

        if op == Symbol("QUOTE"):
            return x[1]

        if op == Symbol("SETQ"):
            # Supports pairs: (setq a 1 b 2 c 3)
            val = NIL
            i = 1
            while i < len(x):
                var = x[i]
                expr = x[i + 1] if i + 1 < len(x) else NIL
                val = lisp_eval(expr, env)
                target_env = env.find(var)
                target_env.set(var, val)
                i += 2
            return val

        if op == Symbol("DEFUN"):
            # (defun name (arg1 arg2 / loc1 loc2) body...)
            fn_name = x[1].name if isinstance(x[1], Symbol) else str(x[1]).upper()
            param_list = x[2] if len(x) > 2 and isinstance(x[2], list) else []
            body = x[3:]

            params = []
            locals_ = []
            target = params
            for p in param_list:
                if p == Symbol("/"):
                    target = locals_
                else:
                    target.append(p if isinstance(p, Symbol) else Symbol(str(p)))

            func = LispFunction(fn_name, params, locals_, body, env)
            env.set(fn_name, func)

            # OpenCAD LISP SPECIAL: If name starts with C:, register as top-level CAD command!
            if fn_name.startswith("C:"):
                cmd_name = fn_name[2:]
                register_cad_command(cmd_name, func)

            return Symbol(fn_name)

        if op == Symbol("LAMBDA"):
            param_list = x[1] if len(x) > 1 and isinstance(x[1], list) else []
            body = x[2:]
            params = []
            locals_ = []
            target = params
            for p in param_list:
                if p == Symbol("/"):
                    target = locals_
                else:
                    target.append(p if isinstance(p, Symbol) else Symbol(str(p)))
            return LispFunction("<lambda>", params, locals_, body, env)

        if op == Symbol("IF"):
            # (if test then [else])
            test = lisp_eval(x[1], env)
            if test is not NIL and test is not False and test is not None:
                x = x[2]
                continue  # Tail-call optimization
            elif len(x) > 3:
                x = x[3]
                continue  # Tail-call optimization
            else:
                return NIL

        if op == Symbol("COND"):
            # (cond (test1 e1...) (test2 e2...) ...)
            for clause in x[1:]:
                if not isinstance(clause, list) or not clause:
                    continue
                test = lisp_eval(clause[0], env)
                if test is not NIL and test is not False and test is not None:
                    if len(clause) == 1:
                        return test
                    # Evaluate intermediate expressions
                    for expr in clause[1:-1]:
                        lisp_eval(expr, env)
                    x = clause[-1]
                    break
            else:
                return NIL
            continue  # Tail-call

        if op == Symbol("WHILE"):
            # (while test body...)
            test_expr = x[1]
            body = x[2:]
            val = NIL
            while True:
                test = lisp_eval(test_expr, env)
                if test is NIL or test is False or test is None:
                    break
                for expr in body:
                    val = lisp_eval(expr, env)
            return val

        if op == Symbol("REPEAT"):
            # (repeat count body...)
            count = int(lisp_eval(x[1], env))
            body = x[2:]
            val = NIL
            for _ in range(count):
                for expr in body:
                    val = lisp_eval(expr, env)
            return val

        if op == Symbol("FOREACH"):
            # (foreach var lst body...)
            var = x[1]
            lst = lisp_eval(x[2], env)
            body = x[3:]
            py_list = lst.to_list() if isinstance(lst, Cons) else (lst if isinstance(lst, (list, tuple)) else [])
            val = NIL
            for item in py_list:
                env.set(var, item)
                for expr in body:
                    val = lisp_eval(expr, env)
            return val

        if op == Symbol("PROGN"):
            for expr in x[1:-1]:
                lisp_eval(expr, env)
            x = x[-1] if len(x) > 1 else NIL
            continue  # Tail-call

        if op == Symbol("AND"):
            for expr in x[1:-1]:
                val = lisp_eval(expr, env)
                if val is NIL or val is False or val is None:
                    return NIL
            x = x[-1] if len(x) > 1 else T
            continue

        if op == Symbol("OR"):
            for expr in x[1:]:
                val = lisp_eval(expr, env)
                if val is not NIL and val is not False and val is not None:
                    return val
            return NIL

        # ── FUNCTION CALL ─────────────────────────────────────────────────────
        proc = lisp_eval(op, env)
        args = [lisp_eval(arg, env) for arg in x[1:]]

        if callable(proc):
            res = proc(*args)
            return res if res is not None else NIL

        raise TypeError(f"'{lisp_repr(op)}' evaluated to '{lisp_repr(proc)}' which is not callable")


# ─────────────────────────────────────────────────────────────────────────────
# 4. Standard OpenCAD LISP Built-ins Library
# ─────────────────────────────────────────────────────────────────────────────

def make_global_env() -> Env:
    """Instantiate a base OpenCAD LISP environment loaded with standard primitives."""
    env = Env()

    # Literals
    env.set(Symbol("NIL"), NIL)
    env.set(Symbol("T"), T)
    env.set(Symbol("PI"), math.pi)

    # ── Arithmetic ──
    env.set(Symbol("+"), lambda *args: sum(args) if args else 0)
    env.set(Symbol("-"), lambda a, b=None: -a if b is None else a - b)
    env.set(Symbol("*"), lambda *args: math.prod(args) if args else 1)
    env.set(Symbol("/"), lambda a, *args: a if not args else math.prod([1/x for x in args]) * a)
    env.set(Symbol("1+"), lambda x: x + 1)
    env.set(Symbol("1-"), lambda x: x - 1)
    env.set(Symbol("ABS"), abs)
    env.set(Symbol("MAX"), max)
    env.set(Symbol("MIN"), min)
    env.set(Symbol("REM"), lambda a, b: a % b)
    env.set(Symbol("SQRT"), math.sqrt)
    env.set(Symbol("EXPT"), math.pow)
    env.set(Symbol("SIN"), math.sin)
    env.set(Symbol("COS"), math.cos)
    env.set(Symbol("ATAN"), lambda y, x=None: math.atan2(y, x) if x is not None else math.atan(y))
    env.set(Symbol("FLOAT"), float)
    env.set(Symbol("FIX"), int)

    # ── Comparisons & Predicates ──
    env.set(Symbol("="), lambda a, b: T if a == b else NIL)
    env.set(Symbol("/="), lambda a, b: T if a != b else NIL)
    env.set(Symbol("<"), lambda a, b: T if a < b else NIL)
    env.set(Symbol("<="), lambda a, b: T if a <= b else NIL)
    env.set(Symbol(">"), lambda a, b: T if a > b else NIL)
    env.set(Symbol(">="), lambda a, b: T if a >= b else NIL)
    env.set(Symbol("EQ"), lambda a, b: T if a is b or a == b else NIL)
    env.set(Symbol("EQUAL"), lambda a, b, fuzz=0.0: T if (abs(a - b) <= fuzz if isinstance(a, (int, float)) and isinstance(b, (int, float)) else a == b) else NIL)
    env.set(Symbol("NOT"), lambda x: T if (x is NIL or x is False or x is None) else NIL)
    env.set(Symbol("NULL"), lambda x: T if (x is NIL or x is False or x is None) else NIL)
    env.set(Symbol("ATOM"), lambda x: T if not isinstance(x, (list, Cons)) else NIL)
    env.set(Symbol("LISTP"), lambda x: T if isinstance(x, (list, Cons)) or x is NIL else NIL)
    env.set(Symbol("NUMBERP"), lambda x: T if isinstance(x, (int, float)) and not isinstance(x, bool) else NIL)
    env.set(Symbol("MINUSP"), lambda x: T if isinstance(x, (int, float)) and x < 0 else NIL)
    env.set(Symbol("ZEROP"), lambda x: T if isinstance(x, (int, float)) and x == 0 else NIL)

    # ── List Operations ──
    def _car(x):
        if isinstance(x, Cons): return x.car
        if isinstance(x, (list, tuple)) and len(x) > 0: return x[0]
        return NIL

    def _cdr(x):
        if isinstance(x, Cons): return x.cdr
        if isinstance(x, (list, tuple)) and len(x) > 1: return list(x[1:])
        return NIL

    def _cons(car, cdr):
        if isinstance(cdr, (list, tuple)):
            return [car] + list(cdr)
        return Cons(car, cdr)

    def _list(*args):
        return list(args)

    def _append(*args):
        out = []
        for a in args:
            if isinstance(a, Cons): out.extend(a.to_list())
            elif isinstance(a, (list, tuple)): out.extend(a)
            elif a is not NIL: out.append(a)
        return out

    def _nth(n, lst):
        py_list = lst.to_list() if isinstance(lst, Cons) else lst
        return py_list[n] if 0 <= n < len(py_list) else NIL

    def _length(lst):
        if isinstance(lst, Cons): return len(lst.to_list())
        if isinstance(lst, (list, tuple)): return len(lst)
        return 0

    def _last(lst):
        py_list = lst.to_list() if isinstance(lst, Cons) else lst
        return py_list[-1] if py_list else NIL

    def _assoc(key, alist):
        py_list = alist.to_list() if isinstance(alist, Cons) else alist
        for item in py_list:
            item_key = item.car if isinstance(item, Cons) else (item[0] if isinstance(item, (list, tuple)) else None)
            if item_key == key:
                return item
        return NIL

    env.set(Symbol("CAR"), _car)
    env.set(Symbol("CDR"), _cdr)
    env.set(Symbol("CAAR"), lambda x: _car(_car(x)))
    env.set(Symbol("CADR"), lambda x: _car(_cdr(x)))
    env.set(Symbol("CDAR"), lambda x: _cdr(_car(x)))
    env.set(Symbol("CDDR"), lambda x: _cdr(_cdr(x)))
    env.set(Symbol("CADDR"), lambda x: _car(_cdr(_cdr(x))))
    env.set(Symbol("CONS"), _cons)
    env.set(Symbol("LIST"), _list)
    env.set(Symbol("APPEND"), _append)
    env.set(Symbol("REVERSE"), lambda lst: list(reversed(lst.to_list() if isinstance(lst, Cons) else lst)))
    env.set(Symbol("LENGTH"), _length)
    env.set(Symbol("NTH"), _nth)
    env.set(Symbol("LAST"), _last)
    env.set(Symbol("ASSOC"), _assoc)
    env.set(Symbol("MEMBER"), lambda item, lst: lst[lst.index(item):] if item in lst else NIL)

    # ── Strings ──
    env.set(Symbol("STRCAT"), lambda *args: "".join(str(a if a is not NIL else "") for a in args))
    env.set(Symbol("STRLEN"), lambda s: len(str(s)))
    env.set(Symbol("SUBSTR"), lambda s, start, length=None: str(s)[start-1:(start-1+length) if length is not None else None])
    env.set(Symbol("STRCASE"), lambda s, which=NIL: str(s).lower() if (which is not NIL and which is not False) else str(s).upper())

    # ── Geometry & Trigonometry ──
    def _distance(p1, p2):
        l1 = p1.to_list() if isinstance(p1, Cons) else p1
        l2 = p2.to_list() if isinstance(p2, Cons) else p2
        return math.dist(l1[:3], l2[:3])

    def _angle(p1, p2):
        l1 = p1.to_list() if isinstance(p1, Cons) else p1
        l2 = p2.to_list() if isinstance(p2, Cons) else p2
        dx = l2[0] - l1[0]
        dy = l2[1] - l1[1]
        a = math.atan2(dy, dx)
        return a if a >= 0 else a + 2 * math.pi

    def _polar(pt, angle, dist):
        l = pt.to_list() if isinstance(pt, Cons) else list(pt)
        x = l[0] + dist * math.cos(angle)
        y = l[1] + dist * math.sin(angle)
        z = l[2] if len(l) > 2 else 0.0
        return [x, y, z]

    env.set(Symbol("DISTANCE"), _distance)
    env.set(Symbol("ANGLE"), _angle)
    env.set(Symbol("POLAR"), _polar)

    # ── CAD Command Invocation (CAD command function) ──
    def _lisp_command(*args):
        if not args:
            return NIL

        global _GLOBAL_CAD_API
        cad = _GLOBAL_CAD_API
        if cad is None:
            cad = sys.modules.get("opencad") or sys.modules.get("oc")

        tokens = []
        for a in args:
            if isinstance(a, Cons):
                a = a.to_list()
            if isinstance(a, (list, tuple)):
                if len(a) >= 2:
                    z_val = a[2] if len(a) > 2 else 0.0
                    tokens.append(f"{a[0]},{a[1]},{z_val}")
                else:
                    tokens.append(str(a))
            elif a is NIL or isinstance(a, LispNil):
                continue
            elif isinstance(a, Symbol):
                tokens.append(a.name)
            else:
                tokens.append(str(a))

        cmd_line = " ".join(tokens)
        if cmd_line.startswith("_."):
            cmd_line = cmd_line[2:]
        elif cmd_line.startswith("_"):
            cmd_line = cmd_line[1:]

        if cad is not None and hasattr(cad, "cmd"):
            return cad.cmd(cmd_line)
        return NIL

    env.set(Symbol("COMMAND"), _lisp_command)

    # ── Standard Entity Creation (entmake) ──
    def _entmake(elist):
        pairs = {}
        items = elist.to_list() if isinstance(elist, Cons) else (elist if isinstance(elist, list) else [])
        for it in items:
            key = None
            val = None
            if isinstance(it, Cons):
                key = it.car
                val = it.cdr.to_list() if isinstance(it.cdr, Cons) else it.cdr
            elif isinstance(it, (list, tuple)):
                if len(it) == 2:
                    key = it[0]
                    val = it[1]
                elif len(it) > 2:
                    key = it[0]
                    val = list(it[1:])
            if key is not None:
                try:
                    pairs[int(key)] = val
                except (ValueError, TypeError):
                    pairs[key] = val

        etype = str(pairs.get(0, "")).upper()
        try:
            log_dir = os.path.join(os.environ.get("APPDATA", tempfile.gettempdir()), "OpenCADStudio")
            os.makedirs(log_dir, exist_ok=True)
            with open(os.path.join(log_dir, "interactive_trace.log"), "a") as f:
                f.write(f"[python] _entmake: etype={etype}, pairs={pairs}\n")
        except Exception:
            pass
        global _GLOBAL_CAD_API
        cad = _GLOBAL_CAD_API
        if cad is None:
            cad = sys.modules.get("opencad") or sys.modules.get("oc")

        if etype == "LINE" and cad is not None:
            p1 = pairs.get(10)
            p2 = pairs.get(11)
            layer = str(pairs.get(8, "0"))
            p1_l = p1.to_list() if isinstance(p1, Cons) else (list(p1) if isinstance(p1, (list, tuple)) else p1)
            p2_l = p2.to_list() if isinstance(p2, Cons) else (list(p2) if isinstance(p2, (list, tuple)) else p2)
            if p1_l is not None and p2_l is not None and hasattr(cad, "add_line"):
                cad.add_line(p1_l, p2_l, layer)
                return elist
        elif etype == "CIRCLE" and cad is not None:
            c = pairs.get(10)
            r = pairs.get(40, 1.0)
            layer = str(pairs.get(8, "0"))
            c_l = c.to_list() if isinstance(c, Cons) else (list(c) if isinstance(c, (list, tuple)) else c)
            if c_l is not None and hasattr(cad, "add_circle"):
                cad.add_circle(c_l, float(r), layer)
                return elist
        elif etype == "POINT" and cad is not None:
            pt = pairs.get(10)
            layer = str(pairs.get(8, "0"))
            pt_l = pt.to_list() if isinstance(pt, Cons) else (list(pt) if isinstance(pt, (list, tuple)) else pt)
            if pt_l is not None and hasattr(cad, "add_point"):
                cad.add_point(pt_l, layer)
                return elist
        elif etype == "TEXT" and cad is not None:
            pt = pairs.get(10, [0.0, 0.0, 0.0])
            txt = str(pairs.get(1, ""))
            h = float(pairs.get(40, 2.5))
            layer = str(pairs.get(8, "0"))
            pt_l = pt.to_list() if isinstance(pt, Cons) else (list(pt) if isinstance(pt, (list, tuple)) else pt)
            if hasattr(cad, "add_text"):
                cad.add_text(pt_l, txt, h, layer=layer)
                return elist
        return elist

    env.set(Symbol("ENTMAKE"), _entmake)
    env.set(Symbol("ENT-MAKE"), _entmake)

    # ── Output & Printing ──
    def _princ(val="", _file=None):
        out = str(val) if isinstance(val, (str, int, float)) else lisp_repr(val)
        sys.stdout.write(out)
        sys.stdout.flush()
        return val

    env.set(Symbol("PRINC"), _princ)
    env.set(Symbol("PRINT"), lambda val: _princ(f"\n{lisp_repr(val)} \n"))
    env.set(Symbol("PROMPT"), lambda msg: _princ(msg))

    # ── Version Query ──
    env.set(Symbol("VER"), lambda: "OpenCAD LISP 2026 (Python fallback engine)")
    env.set(Symbol("VL-VERSION"), lambda: "OpenCAD LISP 2026 (Python fallback engine)")
    env.set(Symbol("LISP-IMPLEMENTATION-TYPE"), lambda: "Python fallback")

    # ── Python Interoperability ──
    env.set(Symbol("PY-EVAL"), lambda code: py_coerce_to_lisp(eval(code)))
    env.set(Symbol("PY-EXEC"), lambda code: exec(code))
    env.set(Symbol("PY-IMPORT"), lambda mod_name: auto_bind_module(sys.modules.get(mod_name) or __import__(mod_name), env, prefix=f"{mod_name}:"))

    return env


# ─────────────────────────────────────────────────────────────────────────────
# 5. Type Coercion & Reflection Bridge
# ─────────────────────────────────────────────────────────────────────────────

def lisp_coerce_to_python(val: Any) -> Any:
    """Coerce an OpenCAD LISP data type to a clean Python equivalent."""
    if val is NIL or isinstance(val, LispNil):
        return None
    if val is T or isinstance(val, LispT):
        return True
    if isinstance(val, Cons):
        # Could be a dotted pair (e.g. DXF code (0 . "LINE")) or a list
        if val.cdr is None or val.cdr is NIL:
            return [lisp_coerce_to_python(val.car)]
        if not isinstance(val.cdr, Cons):
            # True dotted pair -> Python tuple
            return (lisp_coerce_to_python(val.car), lisp_coerce_to_python(val.cdr))
        return [lisp_coerce_to_python(x) for x in val.to_list()]
    if isinstance(val, list):
        return [lisp_coerce_to_python(x) for x in val]
    return val


def py_coerce_to_lisp(val: Any) -> Any:
    """Coerce a Python return value to an OpenCAD LISP equivalent."""
    if val is None:
        return NIL
    if val is True:
        return T
    if val is False:
        return NIL
    if isinstance(val, (list, tuple)):
        # Coordinates and lists become S-expression lists
        return [py_coerce_to_lisp(x) for x in val]
    return val


def create_lisp_wrapper(py_func: Callable) -> Callable:
    """Wrap a Python function with automatic argument and return coercion."""
    def wrapper(*lisp_args):
        # Coerce OpenCAD LISP arguments to Python types
        py_args = [lisp_coerce_to_python(arg) for arg in lisp_args]
        res = py_func(*py_args)
        return py_coerce_to_lisp(res)
    
    wrapper.__doc__ = py_func.__doc__
    return wrapper


OPENCAD_LISP_STANDARD_ALIASES = {
    "get_point": "GETPOINT",
    "getpoint": "GETPOINT",
    "get_dist": "GETDIST",
    "getdist": "GETDIST",
    "get_corner": "GETCORNER",
    "get_string": "GETSTRING",
    "get_int": "GETINT",
    "get_real": "GETREAL",
    "get_kword": "GETKWORD",
    "get_var": "GETVAR",
    "set_var": "SETVAR",
    "alert": "ALERT",
    "ent_get": "ENTGET",
    "ent_make": "ENTMAKE",
    "ent_mod": "ENTMOD",
    "ent_upd": "ENTUPD",
    "ss_get": "SSGET",
    "ss_length": "SSLENGTH",
    "ss_name": "SSNAME",
}


def auto_bind_module(module: Any, env: Env, prefix: str = "CAD:"):
    """
    Automatically inspect a module (like `opencad` or `cad`) and export every
    callable function into the OpenCAD LISP environment:
      1. Snake-case to Kebab-case (e.g. `add_line` -> `(add-line ...)` and `(cad:add-line ...)`)
      2. Standard CAD aliases (e.g. `get_point` -> `(getpoint ...)`)
    """
    for attr_name in dir(module):
        if attr_name.startswith("_") or attr_name.lower() == "command":
            continue
        try:
            attr = getattr(module, attr_name)
        except Exception:
            continue

        if callable(attr):
            wrapper = create_lisp_wrapper(attr)
            kebab = attr_name.replace("_", "-").upper()

            # 1. Direct kebab-case: (add-line ...)
            env.set(Symbol(kebab), wrapper)

            # 2. Prefixed kebab-case: (cad:add-line ...)
            if prefix:
                env.set(Symbol(f"{prefix.upper()}{kebab}"), wrapper)

            # 3. Standard CAD alias if applicable
            clean_name = attr_name.lower()
            if clean_name in OPENCAD_LISP_STANDARD_ALIASES:
                alias = OPENCAD_LISP_STANDARD_ALIASES[clean_name]
                env.set(Symbol(alias), wrapper)


# ─────────────────────────────────────────────────────────────────────────────
# 6. Global CAD Command Registry & Execution
# ─────────────────────────────────────────────────────────────────────────────

_GLOBAL_CAD_API = None
_GLOBAL_ENV = None


def register_cad_command(name: str, func: Callable):
    """Register an OpenCAD LISP C:COMMAND as an OpenCAD Studio command."""
    global _GLOBAL_CAD_API
    cmd_name = name.upper()
    if cmd_name.startswith("C:"):
        cmd_name = cmd_name[2:]
    elif cmd_name.startswith("C_") or cmd_name.startswith("C-"):
        cmd_name = cmd_name[2:]

    # Wrap the function for clean command execution from CAD
    def cmd_wrapper(*args):
        if len(args) == 1 and isinstance(args[0], list):
            call_args = args[0]
        else:
            call_args = list(args)
        return func(*call_args)

    cmd_wrapper.__name__ = cmd_name
    cmd_wrapper.__doc__ = getattr(func, "__doc__", f"OpenCAD LISP command {cmd_name}")

    reg_fn = None
    if _GLOBAL_CAD_API is not None:
        if hasattr(_GLOBAL_CAD_API, "_register_command"):
            reg_fn = _GLOBAL_CAD_API._register_command
        elif hasattr(_GLOBAL_CAD_API, "register_command"):
            reg_fn = _GLOBAL_CAD_API.register_command
    if reg_fn is None:
        for mod_name in ("opencad", "oc", "OPENCAD", "OC"):
            if mod_name in sys.modules:
                mod = sys.modules[mod_name]
                if hasattr(mod, "_register_command"):
                    reg_fn = mod._register_command
                    break
                elif hasattr(mod, "register_command"):
                    reg_fn = mod.register_command
                    break
    if reg_fn is not None:
        try:
            reg_fn(cmd_wrapper, name=cmd_name, description=f"OpenCAD LISP command {cmd_name}")
        except TypeError:
            try:
                reg_fn(cmd_name, cmd_wrapper, f"OpenCAD LISP command {cmd_name}")
            except TypeError:
                try:
                    reg_fn(cmd_name, cmd_wrapper)
                except Exception:
                    pass
    else:
        # Fallback if _register_host_alias is available directly
        for mod_name in ("opencad", "oc"):
            if mod_name in sys.modules:
                mod = sys.modules[mod_name]
                if hasattr(mod, "_register_host_alias"):
                    try:
                        mod._register_host_alias(cmd_name, f"OpenCAD LISP command {cmd_name}")
                    except Exception:
                        pass



def get_global_env() -> Env:
    global _GLOBAL_ENV
    if _GLOBAL_ENV is None:
        _GLOBAL_ENV = make_global_env()
    return _GLOBAL_ENV


import os

_RUST_DLL = None
_RUST_ENGINE = None
_RUST_CMD_CB = None
_RUST_DLL_PATH = None
_CTYPES = None

def _get_rust_engine():
    global _RUST_DLL, _RUST_ENGINE, _RUST_CMD_CB, _RUST_DLL_PATH, _CTYPES
    if _RUST_ENGINE is not None:
        return _RUST_DLL, _RUST_ENGINE

    try:
        import ctypes
        _CTYPES = ctypes
    except Exception:
        return None, None

    base_dir = os.path.dirname(os.path.abspath(__file__))
    appdata = os.environ.get("APPDATA", "")
    plugin_dylib = os.environ.get("OCS_PLUGIN_CDYLIB_PATH", "")
    plugin_dir = os.path.dirname(os.path.abspath(plugin_dylib)) if plugin_dylib else ""

    candidates = [
        # 1. Next to OCS_PLUGIN_CDYLIB_PATH in bin/ subfolder (installed plugin)
        os.path.normpath(os.path.join(plugin_dir, "bin", "opencad_lisp.dll")) if plugin_dir else "",
        # 3. Inside OpenCADStudio AppData directory
        os.path.normpath(os.path.join(appdata, "OpenCADStudio", "plugins", "opencad.pyrunner", "bin", "opencad_lisp.dll")) if appdata else "",
        os.path.normpath(os.path.join(appdata, "OpenCADStudio", "opencad_lisp.dll")) if appdata else "",
        # 4. Dev relative paths from opencad_lisp.py or plugin root
        os.path.normpath(os.path.join(base_dir, "..", "..", "opencad-lisp", "target", "release", "opencad_lisp.dll")),
        os.path.normpath(os.path.join(base_dir, "..", "..", "opencad-lisp", "target", "debug", "opencad_lisp.dll")),
        os.path.normpath(os.path.join(base_dir, "..", "bin", "opencad_lisp.dll")),
        # 5. System PATH or working directory
        "opencad_lisp.dll",
    ]

    for dll_path in candidates:
        if dll_path and os.path.exists(dll_path):
            try:
                dll = _CTYPES.CDLL(dll_path)
                dll.lisp_engine_create.restype = _CTYPES.c_void_p
                dll.lisp_engine_destroy.argtypes = [_CTYPES.c_void_p]

                CMD_CALLBACK = _CTYPES.CFUNCTYPE(_CTYPES.c_int, _CTYPES.c_char_p)
                dll.lisp_engine_set_command_callback.argtypes = [_CTYPES.c_void_p, CMD_CALLBACK]

                engine = dll.lisp_engine_create()
                if engine:
                    @CMD_CALLBACK
                    def on_cmd(cmd_bytes):
                        cmd_str = cmd_bytes.decode("utf-8", errors="replace")
                        if _GLOBAL_CAD_API:
                            parts = cmd_str.strip().split()
                            if parts and parts[0].upper() in ("LINE", "_.LINE") and len(parts) >= 3:
                                try:
                                    p1 = [float(c) for c in parts[1].split(",")]
                                    p2 = [float(c) for c in parts[2].split(",")]
                                    if hasattr(_GLOBAL_CAD_API, "add_line"):
                                        _GLOBAL_CAD_API.add_line(p1, p2)
                                        return 0
                                except Exception:
                                    pass
                            elif parts and parts[0].upper() in ("CIRCLE", "_.CIRCLE") and len(parts) >= 3:
                                try:
                                    pt = [float(c) for c in parts[1].split(",")]
                                    r = float(parts[2])
                                    if hasattr(_GLOBAL_CAD_API, "add_circle"):
                                        _GLOBAL_CAD_API.add_circle(pt[0], pt[1], r)
                                        return 0
                                except Exception:
                                    pass
                            if hasattr(_GLOBAL_CAD_API, "cmd"):
                                try:
                                    _GLOBAL_CAD_API.cmd(cmd_str)
                                    return 0
                                except Exception as e:
                                    sys.stderr.write(f"CAD Command error: {e}\n")
                                    return -1
                        return 0

                    _RUST_CMD_CB = on_cmd
                    dll.lisp_engine_set_command_callback(engine, on_cmd)
                    _RUST_DLL = dll
                    _RUST_ENGINE = engine
                    _RUST_DLL_PATH = dll_path
                    return _RUST_DLL, _RUST_ENGINE
            except Exception as e:
                sys.stderr.write(f"[OpenCAD LISP] Failed loading DLL at {dll_path}: {e}\n")

    return None, None


def is_rust_engine_loaded() -> bool:
    """Return True if the native Rust opencad-lisp engine is active."""
    dll, engine = _get_rust_engine()
    return dll is not None and engine is not None


def get_engine_info() -> Dict[str, Any]:
    """Return information about the active OpenCAD LISP engine."""
    dll, engine = _get_rust_engine()
    if dll and engine:
        return {
            "engine": "native_rust",
            "implementation": "opencad-lisp (Rust)",
            "version": "0.1.0",
            "dll_path": _RUST_DLL_PATH,
            "is_fallback": False,
        }
    return {
        "engine": "python_fallback",
        "implementation": "OpenCAD LISP (Python)",
        "version": "1.0",
        "dll_path": None,
        "is_fallback": True,
    }


def init_opencad_lisp(cad_api: Any) -> Env:
    """Initialize the OpenCAD LISP engine with the live OpenCAD Python API."""
    global _GLOBAL_CAD_API, _GLOBAL_ENV
    _GLOBAL_CAD_API = cad_api
    env = get_global_env()
    auto_bind_module(cad_api, env, prefix="CAD:")
    # Initialize the Rust DLL engine if available
    _get_rust_engine()
    return env


def eval_rust_lisp(code: str) -> Any:
    """Evaluate OpenCAD LISP code using the native Rust opencad_lisp.dll engine."""
    dll, engine = _get_rust_engine()
    if not (dll and engine and _CTYPES):
        raise RuntimeError("opencad_lisp.dll is not loaded")
    buf = _CTYPES.create_string_buffer(65536)
    code_bytes = code.encode("utf-8")
    res = dll.lisp_eval_string(_CTYPES.c_void_p(engine), code_bytes, buf, 65536)
    if res == 0:
        # Register any (defun c:NAME ...) commands
        for m in re.finditer(r"\(\s*defun\s+(?:c:|C:)([\w\-]+)", code):
            cmd_name = m.group(1).upper()
            def make_wrapper(cn):
                def _wrapper(*args):
                    return eval_rust_lisp(f"(c:{cn})")
                return _wrapper
            register_cad_command(cmd_name, make_wrapper(cmd_name))

        val_str = buf.value.decode("utf-8", errors="replace")
        if val_str == "nil":
            return NIL
        elif val_str == "T":
            return T
        return val_str
    else:
        err_msg = buf.value.decode("utf-8", errors="replace")
        raise RuntimeError(f"OpenCAD LISP Error: {err_msg}")


def eval_lisp(code: str, env: Optional[Env] = None) -> Any:
    """Evaluate an OpenCAD LISP code string and return the result."""
    # 1. Try the high-speed native Rust OpenCAD LISP engine first
    dll, engine = _get_rust_engine()
    code_lower = code.lower()
    interactive_keywords = ("getpoint", "getdist", "getcorner", "getkword", "getstring", "getint", "getreal", "cad:")
    needs_python_interactive = any(kw in code_lower for kw in interactive_keywords)

    if dll and engine and not needs_python_interactive:
        try:
            return eval_rust_lisp(code)
        except Exception:
            # Fall back to Python interpreter if Rust engine encounters unhandled construct
            pass

    # 2. Fallback to Python interpreter (for Python reflection / custom bindings)
    if env is None:
        env = get_global_env()
    exprs = parse_all(code)
    res = NIL
    for expr in exprs:
        res = lisp_eval(expr, env)
    return res


def load_lisp_file(file_path: str, env: Optional[Env] = None) -> Any:
    """Load and execute an OpenCAD LISP file (.lsp)."""
    with open(file_path, "r", encoding="utf-8", errors="replace") as f:
        code = f.read()
    return eval_lisp(code, env)

