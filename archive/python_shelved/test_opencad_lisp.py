import os
import sys

# Ensure src is importable
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "src")))

import opencad_lisp
from opencad_lisp import NIL, T, Symbol, Cons, eval_lisp, get_global_env, auto_bind_module


def test_core_math():
    print("Testing core math...")
    assert eval_lisp("(+ 1 2 3 4)") == 10
    assert eval_lisp("(- 10 3)") == 7
    assert eval_lisp("(* 2 3 4)") == 24
    assert eval_lisp("(/ 20 4 2)") == 2.5
    assert eval_lisp("(1+ 9)") == 10
    assert eval_lisp("(1- 9)") == 8
    assert eval_lisp("(rem 10 3)") == 1
    assert eval_lisp("(abs -42)") == 42
    print("  -> Passed!")


def test_core_logic_and_conditions():
    print("Testing logic & conditions...")
    assert eval_lisp("(= 5 5)") == T
    assert eval_lisp("(= 5 6)") == NIL
    assert eval_lisp("(/= 5 6)") == T
    assert eval_lisp("(< 3 5)") == T
    assert eval_lisp("(not nil)") == T
    assert eval_lisp("(not T)") == NIL

    assert eval_lisp("(if (= 1 1) 'yes 'no)") == Symbol("YES")
    assert eval_lisp("(if (= 1 2) 'yes 'no)") == Symbol("NO")

    cond_code = """
    (cond
      ((= 1 2) "first")
      ((= 2 2) "second")
      (T "default"))
    """
    assert eval_lisp(cond_code) == "second"
    print("  -> Passed!")


def test_variables_and_scoping():
    print("Testing variables & scoping...")
    eval_lisp("(setq a 100 b 200)")
    assert eval_lisp("(+ a b)") == 300

    # Local variable scoping in defun: (defun name (arg1 / loc1) ...)
    fn_code = """
    (defun test-scope (x / y)
      (setq y 50)
      (+ x y a))
    """
    eval_lisp(fn_code)
    assert eval_lisp("(test-scope 10)") == 160
    # y should not leak to global scope
    assert eval_lisp("y") == NIL
    print("  -> Passed!")


def test_recursion():
    print("Testing recursive defun (factorial)...")
    code = """
    (defun fact (n)
      (if (<= n 1)
        1
        (* n (fact (1- n)))))
    """
    eval_lisp(code)
    assert eval_lisp("(fact 5)") == 120
    assert eval_lisp("(fact 6)") == 720
    print("  -> Passed!")


def test_lists_and_dxf_dotted_pairs():
    print("Testing lists and DXF dotted pairs...")
    assert eval_lisp("(car '(1 2 3))") == 1
    assert eval_lisp("(cdr '(1 2 3))") == [2, 3]
    assert eval_lisp("(cadr '(10 20 30))") == 20
    assert eval_lisp("(caddr '(10 20 30))") == 30

    # DXF association list: ((0 . "LINE") (10 0 0 0) (11 10 10 0))
    dxf_code = """
    (setq edata '((0 . "LINE") (8 . "WALLS") (10 0.0 0.0 0.0) (11 10.0 20.0 0.0)))
    """
    eval_lisp(dxf_code)
    assert eval_lisp("(cdr (assoc 0 edata))") == "LINE"
    assert eval_lisp("(cdr (assoc 8 edata))") == "WALLS"
    assert eval_lisp("(cdr (assoc 10 edata))") == [0.0, 0.0, 0.0]
    print("  -> Passed!")


def test_command_registration():
    print("Testing C:COMMAND registration...")
    registered = {}

    class MockCadApi:
        def register_command(self, name, func, doc):
            registered[name] = func

    opencad_lisp.init_opencad_lisp(MockCadApi())

    code = """
    (defun c:draw_box ()
      (setq msg "Box drawn successfully!")
      msg)
    """
    eval_lisp(code)

    assert "DRAW_BOX" in registered
    # Execute the registered command callable directly
    res = registered["DRAW_BOX"]()
    assert res == "Box drawn successfully!"
    print("  -> Passed!")


def test_automatic_python_reflection():
    print("Testing automatic Python API reflection into OpenCAD LISP...")

    created_lines = []
    created_circles = []
    sysvars = {"OSMODE": 7}

    class LiveCadApi:
        """Mock representation of OpenCADStudio Python API."""
        def add_line(self, p1, p2, layer="0"):
            created_lines.append((p1, p2, layer))
            return 42  # entity handle

        def draw_circle(self, center, radius):
            created_circles.append((center, radius))
            return 43

        def get_point(self, prompt=""):
            return (15.0, 25.0, 0.0)

        def get_var(self, name):
            return sysvars.get(name.upper())

        def set_var(self, name, val):
            sysvars[name.upper()] = val
            return val

        # A brand-new custom function added to Python!
        def custom_super_feature(self, factor, name):
            return f"Processed {name} with factor {factor * 2}"

    api = LiveCadApi()
    env = opencad_lisp.init_opencad_lisp(api)

    # 1. Test kebab-case automatic reflection:
    # add_line -> (add-line ...) and (cad:add-line ...)
    res1 = eval_lisp('(add-line \'(0 0 0) \'(100 50 0) "DIM")', env)
    assert res1 == 42
    assert created_lines[-1] == ([0, 0, 0], [100, 50, 0], "DIM")

    res2 = eval_lisp('(cad:add-line \'(10 10 0) \'(20 20 0))', env)
    assert res2 == 42
    assert created_lines[-1] == ([10, 10, 0], [20, 20, 0], "0")

    # 2. Test standard CAD alias:
    # get_point -> (getpoint ...)
    pt = eval_lisp('(getpoint "\nPick a point: ")', env)
    assert pt == [15.0, 25.0, 0.0]

    # 3. Test getvar and setvar
    assert eval_lisp('(getvar "OSMODE")', env) == 7
    eval_lisp('(setvar "OSMODE" 3)', env)
    assert sysvars["OSMODE"] == 3

    # 4. Test newly added custom python function:
    # custom_super_feature -> (custom-super-feature ...)
    custom_res = eval_lisp('(custom-super-feature 5 "Bearing_Pad")', env)
    assert custom_res == "Processed Bearing_Pad with factor 10"

    print("  -> Passed!")


def test_python_interop_inside_lisp():
    print("Testing direct Python evaluation inside OpenCAD LISP...")
    assert eval_lisp('(py-eval "2 ** 10")') == 1024
    assert eval_lisp('(py-eval "\'hello \'.upper() + \'world\'")') == "HELLO world"
    print("  -> Passed!")


if __name__ == "__main__":
    test_core_math()
    test_core_logic_and_conditions()
    test_variables_and_scoping()
    test_recursion()
    test_lists_and_dxf_dotted_pairs()
    test_command_registration()
    test_automatic_python_reflection()
    test_python_interop_inside_lisp()
    print("\nALL 7 OPENCAD LISP TESTS PASSED 100%!")
