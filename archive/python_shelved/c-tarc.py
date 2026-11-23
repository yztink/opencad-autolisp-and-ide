"""
Tangent Arc (TARC) tool for OpenCAD Studio.
Translated from OpenCAD LISP c:TARC.

Usage:
  1. In OpenCAD Studio command line: type `TARC` (or `PY tarc`)
  2. Select an existing LINE or ARC near the endpoint to continue from.
  3. Pick sequential points in the viewport to generate smooth tangent arcs.
  4. Press Enter or Esc to finish.
"""

import math
import opencad as oc

def v_add(a, b):
    return (a[0] + b[0], a[1] + b[1], (a[2] if len(a) > 2 else 0.0) + (b[2] if len(b) > 2 else 0.0))

def v_sub(a, b):
    return (a[0] - b[0], a[1] - b[1], (a[2] if len(a) > 2 else 0.0) - (b[2] if len(b) > 2 else 0.0))

def v_scale(v, s):
    return (v[0] * s, v[1] * s, (v[2] if len(v) > 2 else 0.0) * s)

def v_len(v):
    return math.hypot(v[0], v[1])

def v_unit(v):
    l = v_len(v)
    if l > 1e-12:
        return (v[0] / l, v[1] / l, 0.0)
    return (0.0, 0.0, 0.0)

def v_dot(a, b):
    return a[0] * b[0] + a[1] * b[1]

def v_left(v):
    """Rotate vector 90 degrees Counter-Clockwise in XY plane."""
    return (-v[1], v[0], 0.0)

def v_right(v):
    """Rotate vector 90 degrees Clockwise in XY plane."""
    return (v[1], -v[0], 0.0)

def dist_2d(p1, p2):
    return math.hypot(p2[0] - p1[0], p2[1] - p1[1])

def angle_2d(origin, target):
    """Angle from origin to target in radians [0, 2*pi)."""
    a = math.atan2(target[1] - origin[1], target[0] - origin[0])
    if a < 0.0:
        a += 2.0 * math.pi
    return a

@oc.command("TARC")
def tarc(*args):
    """Create tangent arcs continuing from an existing LINE or ARC."""
    picked = oc.get_entity("Select a LINE or ARC: ")
    if not picked:
        oc.info("Nothing selected.")
        return

    ent_handle, click_pt = picked
    data = oc.get_entity_data(ent_handle)
    if not data:
        oc.info("Could not read selected entity data.")
        return

    typ = data.get("type", "").upper()
    if typ not in ("LINE", "ARC"):
        oc.error("Object must be a LINE or ARC.")
        return

    # Extract endpoints
    if typ == "LINE":
        p1 = data["start"]
        p2 = data["end"]
    elif typ == "ARC":
        center = data["center"]
        radius = data["radius"]
        a1 = data["start_angle"]
        a2 = data["end_angle"]
        p1 = (
            center[0] + radius * math.cos(a1),
            center[1] + radius * math.sin(a1),
            0.0,
        )
        p2 = (
            center[0] + radius * math.cos(a2),
            center[1] + radius * math.sin(a2),
            0.0,
        )

    # Determine which endpoint was clicked closer to
    d1 = dist_2d(click_pt, p1)
    d2 = dist_2d(click_pt, p2)

    if d1 < d2:
        # Continue from P1
        start = p1
        if typ == "LINE":
            # Tangent direction away from P1: (p1 - p2)
            tangent = v_unit(v_sub(p1, p2))
        else:
            # ARC: Tangent direction going backward from start point
            radial = v_unit(v_sub(p1, center))
            tangent = v_right(radial)
    else:
        # Continue from P2
        start = p2
        if typ == "LINE":
            # Tangent direction away from P2: (p2 - p1)
            tangent = v_unit(v_sub(p2, p1))
        else:
            # ARC: Tangent direction going forward from end point
            radial = v_unit(v_sub(p2, center))
            tangent = v_left(radial)

    oc.undo("Tangent Arcs (TARC)")

    # Sequentially create tangent arcs
    while True:
        next_pt = oc.get_point(
            "Specify endpoint of tangent arc <Enter to finish>: ",
            rubber_band=("tangent_arc", start, tangent),
        )
        if not next_pt:
            break

        q = v_sub(next_pt, start)
        qlen = v_len(q)
        if qlen < 1e-10:
            oc.info("Points are too close.")
            continue

        # Normal perpendicular to current tangent
        normal = v_left(tangent)
        dot = v_dot(q, normal)

        if abs(dot) < 1e-10:
            oc.info("The selected point lies on the tangent line; an arc cannot be created.")
            continue

        # Center of tangent arc:
        # h = |q|^2 / (2 * (q . normal))
        h = (qlen * qlen) / (2.0 * dot)
        new_center = v_add(start, v_scale(normal, h))
        new_radius = dist_2d(start, new_center)

        # In CAD / DXF, arcs are defined counter-clockwise from start_angle to end_angle.
        # If h > 0, the arc curves left (CCW from start to next_pt).
        # If h < 0, the arc curves right (CW from start to next_pt, so CCW from next_pt to start).
        a_start = angle_2d(new_center, start)
        a_next = angle_2d(new_center, next_pt)

        if h > 0.0:
            new_a1 = a_start
            new_a2 = a_next
        else:
            new_a1 = a_next
            new_a2 = a_start

        # Create ARC in active drawing
        oc.add_arc(new_center, new_radius, new_a1, new_a2)

        # The end of this arc becomes the new starting point
        start = next_pt

        # Radius vector at the new endpoint
        radial = v_unit(v_sub(next_pt, new_center))

        # Update tangent direction for the next arc
        if h > 0.0:
            tangent = v_left(radial)
        else:
            tangent = v_right(radial)

def run(args):
    tarc(*args)

if __name__ == "__main__":
    tarc()
