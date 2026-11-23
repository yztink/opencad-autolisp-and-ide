;; =============================================================================
;; OpenCAD Studio Sample OpenCAD LISP Routine
;; Demonstrates standard OpenCAD LISP syntax + entity creation via entmake.
;; =============================================================================

;; 1. Standard OpenCAD LISP function: Draw a circle with a center and radius
(defun draw-target (center radius / p1 p2 p3 p4)
  (princ "\nDrawing crosshair target...")
  
  ;; Create target circle using entmake
  (entmake (list '(0 . "CIRCLE")
                 (cons 10 center)
                 (cons 40 radius)))
  
  ;; Calculate crosshair endpoints in a single setq block
  (setq p1 (polar center 0.0 (* radius 1.25))
        p2 (polar center pi (* radius 1.25))
        p3 (polar center (/ pi 2.0) (* radius 1.25))
        p4 (polar center (* pi 1.5) (* radius 1.25)))
  
  ;; Create crosshair lines using entmake
  (entmake (list '(0 . "LINE")
                 (cons 10 p1)
                 (cons 11 p2)))
  (entmake (list '(0 . "LINE")
                 (cons 10 p3)
                 (cons 11 p4)))
  (princ "\nTarget complete!")
)

;; 2. Register a top-level OpenCAD Studio command: TARGET
;; Type 'TARGET' in OpenCAD Studio to invoke this command directly!
(defun c:target (/ pt rad)
  (princ "\n=== OpenCAD LISP TARGET Command ===")
  (setq pt  (getpoint "\nSpecify center point: ")
        pt  (if (null pt) '(0.0 0.0 0.0) pt)
        rad (getdist pt "\nSpecify target radius <25.0>: ")
        rad (if (null rad) 25.0 rad))
  (draw-target pt rad)
  (princ)
)

(princ "\nLoaded sample_demo.lsp. Type TARGET to run.\n")
