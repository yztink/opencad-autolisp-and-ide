# OpenCAD Studio Python API Reference

This document provides a comprehensive reference for writing Python scripts and commands in **OpenCAD Studio** using the `opencad` module provided by the `opencad-py-runner` plugin.

---

## 1. Overview & Execution Context

Python scripts can be executed inside OpenCAD Studio in several ways:
- **Direct CLI Execution:** `PY <script_name> [args...]`
- **One-liner execution:** `PYRUN <code>` (e.g. `PYRUN import opencad as oc; oc.add_circle(0, 0, 0, 25)`)
- **Auto-registered Commands:** Any script placed in `~/.config/OpenCADStudio/scripts/` starting with `c-` or `c_` is automatically registered as a top-level CAD command (e.g., `c-dl.py` or `c-draw_line.py` registers as the command `DL` or `DRAW_LINE`).
- **Headless Automation (`OpenCADStudio --serve`):** Fully supported with coordinates and entity selection supplied over stdio or JSON-RPC.

### Namespaces
In your scripts, functions can be accessed via:
```python
import opencad
import opencad as oc
```
Additionally, all functions and their uppercase aliases (e.g., `add_line` and `ADD_LINE`, `get_point` and `GET_POINT`) are automatically injected into the global namespace.

---

## 2. Interactive Input (Point & Object Picking)

| Method | Arguments | Returns | Description |
| :--- | :--- | :--- | :--- |
| `get_point(prompt=None, base_point=None, rubber_band=None, color=None)` | `prompt: str` *(optional)*<br>`base_point: tuple \| list` *(optional)*<br>`rubber_band: tuple \| list` *(optional)*<br>`color: str \| int \| tuple` *(optional)* | `(x, y, z)` or `None` | Suspends script execution and prompts the user/CLI to pick a 3D coordinate point. Renders real-time native CAD rubber-band preview wires matching cursor movements. Returns `None` if cancelled (`Esc`). |
| `get_dist(prompt_or_base=None, base_point_or_prompt=None)` | `prompt: str` *(optional)*<br>`base_point: tuple \| list` *(optional)* | `float` or `None` | Measures distance between two points (or from a given base point with a rubber-band line to the cursor). Returns `None` if cancelled. Also available in OpenCAD LISP as `(getdist [pt] [prompt])`. |
| `set_rubber_band(spec=None, color=None)` | `spec: tuple \| list \| str \| None`<br>`color: str \| int \| tuple` *(optional)* | `None` | Dynamically sets or clears (`None`) the real-time rubber band preview geometry. |
| `get_entity(prompt=None)` | `prompt: str` *(optional)* | `(handle, (x, y, z))` or `None` | Prompts the user to pick an entity on canvas or enter a handle. Returns `(handle, (x, y, z))` or `None` if cancelled. |
| `get_selection(prompt=None)` | `prompt: str` *(optional)* | `list[int]` or `None` | Prompts user to select entities until `Enter` (or empty string `""` in headless mode). If `prompt` is omitted and entities are already selected, returns them immediately. |
| `get_preselection()` | *None* | `list[int]` | Queries the handles of all currently selected entities in the active drawing tab without prompting. |

### Real-Time Rubber-Band Preview Options
`get_point()` supports real-time visual feedback rendered at 60–120 FPS directly inside OpenCAD Studio's render loop without IPC latency:
- **Line rubber-band (CAD style):**
  - `oc.get_point("Next point: ", base_point=p1)` or `oc.get_point("Next point: ", p1)`
  - Draws a solid preview line connecting `base_point` to the moving cursor.
- **Circle rubber-band:**
  - `oc.get_point("Radius: ", rubber_band=("circle", center))`
  - Dynamically draws a preview circle centered at `center` with radius updating to the cursor distance.
- **3-Point Arc rubber-band:**
  - `oc.get_point("Third point: ", rubber_band=("arc_3p", p1, p2))`
  - Dynamically circumscribes and draws the arc passing through `p1`, `p2`, and the cursor.
- **Center-Start Arc rubber-band:**
  - `oc.get_point("Arc end: ", rubber_band=("arc_center_start", center, start))`
  - Draws the arc starting at `start` centered at `center` and sweeping to the cursor angle.
- **Tangent Arc rubber-band:**
  - `oc.get_point("Next point: ", rubber_band=("tangent_arc", start, tangent))`
  - Solves and dynamically draws the smooth tangent arc originating at `start` in direction `tangent` ending at the cursor.
- **Custom wire list:**
  - `oc.get_point("Next point: ", rubber_band=("custom", [wire1_points, wire2_points]))`
- **Preview Color:**
  - Defaults to native OpenCAD Studio cyan (`[0.0, 1.0, 1.0, 1.0]`).
  - Pass `color="red"`, `color=1` (ACI), `color=(255, 128, 0)`, or `color="#ff0000"` to customize the preview wire color.

---

## 2.1 Declarative Modal Dialogs & GUI Controls

Plugins can display native in-canvas modal dialogs using declarative Python dictionaries or keyword arguments. Dialogs are rendered natively using OpenCAD Studio's Iced UI theme and seamlessly integrate with the interactive command system.

### Methods & Helpers

| Method | Arguments | Returns | Description |
| :--- | :--- | :--- | :--- |
| `dialog(title, *args, submit_label="OK", cancel_label="Cancel", **kwargs)` | `title: str`<br>`*args: _Row \| _Field`<br>`submit_label: str`<br>`cancel_label: str`<br>`**kwargs: control values or _Field` | `AttrDict` or `None` | Displays a native modal dialog box. Keyword arguments are placed on individual vertical rows; `oc.row(...)` groups controls horizontally. Returns an `AttrDict` mapping control IDs to entered values, or `None` if cancelled. |
| `row(*args, **kwargs)` | `*args: _Field \| _Row`<br>`**kwargs: name=value \| _Field` | `_Row` | Groups multiple controls into a single horizontal row with side-by-side positioning. |
| `text(default="", label=None)` | `default: str`<br>`label: str \| None` | `_TextField` | Declares a single-line text input field control. |
| `checkbox(default=False, label=None)` | `default: bool`<br>`label: str \| None` | `_CheckboxField` | Declares a boolean checkbox toggle control. |
| `number(default=0.0, min=None, max=None, step=None, label=None)` | `default: float`<br>`min: float \| None`<br>`max: float \| None`<br>`step: float \| None`<br>`label: str \| None` | `_NumberField` | Declares a numeric input control with optional bounding constraints. |
| `choice(options, default=None, label=None)` | `options: list[str]`<br>`default: str \| int \| None`<br>`label: str \| None` | `_ChoiceField` | Declares a dropdown selection pick list control. `default` can be an option string or 0-based index. |
| `file(label=None, extensions=None, save=False, default="")` | `label: str \| None`<br>`extensions: list[str] \| None`<br>`save: bool`<br>`default: str` | `_FileField` | Declares an OS file picker input with a "Browse..." button. Supports open or save mode. |
| `color(default="red", label=None)` | `default: str \| tuple \| list`<br>`label: str \| None` | `_ColorField` | Declares a color picker preview control with standard color name or RGB values. |
| `alert(message, title="Notice")` | `message: str`<br>`title: str` | `AttrDict` or `None` | Displays an informational alert modal dialog with an "OK" button. |
| `confirm(question, title="Confirm")` | `question: str`<br>`title: str` | `bool` | Displays a confirmation modal dialog with "Yes" and "No" buttons. Returns `True` if confirmed, `False` if cancelled. |

### Type Inference

When specifying controls directly via keyword arguments (e.g. `oc.dialog("Config", radius=10.0, fill=True)`), types are inferred automatically:
- `bool` &rarr; Checkbox
- `int` or `float` &rarr; Number input
- `list` or `tuple` of strings &rarr; Dropdown pick-list
- `str` &rarr; Single-line text input
- Field helpers &rarr; Explicit typed controls with metadata:
  - `oc.text(default="", label=None)`: Text input box.
  - `oc.number(default=0.0, min=None, max=None, step=None, label=None)`: Bounded/stepped number input.
  - `oc.checkbox(default=False, label=None)`: Boolean checkbox toggle.
  - `oc.choice(options, default=None, label=None)`: Dropdown selection. `default` can be an exact string or an integer index (`default=0`).
  - `oc.color(default="red", label=None)`: Color picker preview and selection.
  - `oc.file(label=None, extensions=None, save=False, default="")`: OS file open/save picker.
  - `oc.fields(title=None, *args, **kwargs)`: Grouped property section with aligned 2-column key-value layout.
  - `oc.table(headers, rows=None, id=None, label=None, allow_add=True, allow_delete=True)`: Interactive editable tabular data grid.
  - `oc.row(*args, **kwargs)`: Horizontal side-by-side row of controls.
  - `oc.separator()`: Visual dividing line (horizontal across dialog, or vertical inside `oc.row`).

### Code Examples

#### Simple Form
```python
import opencad as oc

cfg = oc.dialog(
    "Circle Parameters",
    radius=25.0,
    layer="WALLS",
    filled=False,
)
if not cfg:
    print("User cancelled dialog")
    return

oc.add_circle((0, 0), cfg.radius)
```

#### Property Groups (`oc.fields`)
```python
import opencad as oc

cfg = oc.dialog(
    "Shaft Specifications",
    oc.fields(
        "Primary Dimensions",
        diameter=oc.number(40.0, min=5.0, label="Shaft Diameter"),
        length=oc.number(120.0, min=10.0, label="Total Length"),
    ),
    oc.fields(
        "Material & Finish",
        material=oc.choice(["AISI 1045", "Stainless 304"], default=0, label="Alloy"),
        heat_treated=oc.checkbox(True, label="Heat Treated"),
    ),
)
if cfg:
    # Values can be accessed through group names or directly:
    print(f"Diameter: {cfg.primary_dimensions.diameter}, Material: {cfg.material}")
```

#### Editable Data Table (`oc.table`)
```python
import opencad as oc

res = oc.dialog(
    "Bill of Materials",
    bom=oc.table(
        headers=["Part Name", "Qty", "Material"],
        rows=[
            ["Mounting Bracket", "2", "Steel 4140"],
            ["Hex Bolt M8x30", "8", "Grade 8.8"],
        ],
        allow_add=True,
        allow_delete=True,
    ),
    width=600.0,
)
if res:
    for row in res.bom:
        print(f"Part: {row[0]}, Qty: {row[1]}, Material: {row[2]}")
```

#### Advanced Layout with Horizontal Rows (`oc.row`)
```python
import opencad as oc

cfg = oc.dialog(
    "Slot Generator",
    # Horizontal row with Length and Width side-by-side:
    oc.row(
        length=oc.number(100.0, min=1.0, max=1000.0, label="Slot Length"),
        width=oc.number(20.0, min=1.0, max=100.0, label="Slot Width"),
    ),
    # Dropdown choice and color picker:
    material=oc.choice(["Steel", "Aluminum", "Brass"], default=1), # index 1 = Aluminum
    outline_color=oc.color("cyan"),
    # File picker row:
    save_path=oc.file(label="Export Spec", extensions=["txt", "csv"], save=True),
    submit_label="Generate Slot",
)

if cfg:
    print(f"Generating {cfg.material} slot: {cfg.length} x {cfg.width}")
```

#### Alert and Confirmation Dialogs
```python
import opencad as oc

if oc.confirm("Do you want to clear all entities on layer 0?"):
    # Perform operation
    oc.alert("Entities successfully removed.", title="Operation Complete")
```

---

## 3. Entity Creation

All entity creation functions return the unique 64-bit integer handle (`int`) assigned by the CAD document. Coordinates are accepted as 2D `(x, y)` or 3D `(x, y, z)` tuples or lists (with `z` defaulting to `0.0` when omitted).

| Method | Arguments | Returns | Description |
| :--- | :--- | :--- | :--- |
| `add_line(p1, p2, layer="0", color=None)` | `p1, p2: tuple \| list`<br>`layer: str`<br>`color: int \| str \| tuple` | `int` (handle) | Creates a 3D line segment between `p1` and `p2`. |
| `add_circle(center, radius, layer="0", color=None)` | `center: tuple \| list`<br>`radius: float`<br>`layer: str`<br>`color: int \| str \| tuple` | `int` (handle) | Creates a circle centered at `center` with the given radius. |
| `add_arc(center, radius, start_angle, end_angle, layer="0", color=None)` | `center: tuple \| list`<br>`radius: float`<br>`start_angle, end_angle: float`<br>`layer: str`<br>`color: int \| str \| tuple` | `int` (handle) | Creates an arc centered at `center` between `start_angle` and `end_angle` (in radians). |
| `add_ellipse(center, major_axis, minor_axis=None, ratio=None, start_param=0.0, end_param=2pi, layer="0", color=None)` | `center, major_axis: tuple \| list`<br>`minor_axis: float \| tuple`<br>`ratio: float`<br>`layer: str`<br>`color: int \| str \| tuple` | `int` (handle) | Creates an ellipse. Supports direct `ratio` ($0 < \text{ratio} \le 1.0$) or scalar/vector `minor_axis`. |
| `add_polyline(points, is_closed=False, layer="0", color=None)` | `points: list[tuple \| list]`<br>`is_closed: bool`<br>`layer: str`<br>`color: int \| str \| tuple` | `int` (handle) | Creates a 2D polyline. Vertices can be `(x, y)` for straight segments or `(x, y, bulge)` for arc segments ($\text{bulge} = \tan(\theta/4)$). |
| `add_point(pt, layer="0", color=None)` | `pt: tuple \| list`<br>`layer: str`<br>`color: int \| str \| tuple` | `int` (handle) | Creates a point entity at `pt`. |
| `add_text(text, pt, height=2.5, rotation=0.0, style="STANDARD", layer="0", color=None)` | `text: str`<br>`pt: tuple \| list`<br>`height: float`<br>`rotation: float`<br>`style: str`<br>`layer: str`<br>`color: int \| str \| tuple` | `int` (handle) | Creates single-line text with height, rotation (radians), and style. |
| `add_mtext(insertion_point, text, height=2.5, width=0.0, rotation=0.0, style="STANDARD", layer="0", color=None)` | `insertion_point: tuple \| list`<br>`text: str`<br>`height, width, rotation: float`<br>`style: str`<br>`layer: str`<br>`color: int \| str \| tuple` | `int` (handle) | Creates multi-line formatted text. |
| `add_ray(base_point, direction_point, layer="0", color=None)` | `base_point, direction_point: tuple \| list`<br>`layer: str`<br>`color: int \| str \| tuple` | `int` (handle) | Creates a semi-infinite ray from `base_point` through `direction_point`. |
| `add_xline(base_point, direction_point, layer="0", color=None)` | `base_point, direction_point: tuple \| list`<br>`layer: str`<br>`color: int \| str \| tuple` | `int` (handle) | Creates an infinite construction line through `base_point` and `direction_point`. |
| `add_solid(p1, p2, p3, p4=None, layer="0", color=None)` | `p1, p2, p3: tuple \| list`<br>`p4: tuple \| list \| None`<br>`layer: str`<br>`color: int \| str \| tuple` | `int` (handle) | Creates a filled 2D polygon solid (triangle if `p4=None`, quad if `p4` provided). |

### Color Formats Accepted
- **Standard Color Index (ACI):** Integer `1` to `255` (e.g. `1` = red, `2` = yellow, `3` = green, `4` = cyan, `5` = blue, `6` = magenta, `7` = white).
- **Color Name:** Case-insensitive string: `"red"`, `"yellow"`, `"green"`, `"cyan"`, `"blue"`, `"magenta"`, `"white"`, `"bylayer"`, `"byblock"`.
- **RGB Tuple:** 3-tuple `(r, g, b)` with integer values `0`–`255` (e.g. `(255, 128, 0)`).

---

## 4. 3D Solids & Booleans

| Method | Arguments | Returns | Description |
| :--- | :--- | :--- | :--- |
| `add_box(corner, length, width, height, rotation=0.0, layer="0", color=None)` | `corner: tuple \| list`<br>`length, width, height: float`<br>`rotation: float`<br>`layer: str` | `int` (handle) | Creates an axis-aligned solid box placed from `corner`, with optional Z-axis rotation angle in radians. |
| `add_wedge(corner, length, width, height, rotation=0.0, layer="0", color=None)` | `corner: tuple \| list`<br>`length, width, height: float`<br>`rotation: float`<br>`layer: str` | `int` (handle) | Creates a right triangular prism wedge placed from `corner`, with optional Z-axis rotation angle. |
| `add_cylinder(center, radius, height=None, axis_endpoint=None, layer="0", color=None)` | `center: tuple \| list`<br>`radius: float`<br>`height: float \| None`<br>`axis_endpoint: tuple \| list \| None`<br>`layer: str` | `int` (handle) | Creates a solid cylinder. If `axis_endpoint` is specified, automatically orients and tilts the cylinder from `center` toward `axis_endpoint`. |
| `add_sphere(center, radius, layer="0", color=None)` | `center: tuple \| list`<br>`radius: float`<br>`layer: str` | `int` (handle) | Creates a solid 3D sphere at `center`. |
| `add_torus(center, major_radius, minor_radius, axis_endpoint=None, layer="0", color=None)` | `center: tuple \| list`<br>`major_radius, minor_radius: float`<br>`axis_endpoint: tuple \| list \| None`<br>`layer: str` | `int` (handle) | Creates a solid torus with optional axis alignment toward `axis_endpoint`. |
| `add_pyramid(center, base_radius, height=None, sides=4, top_radius=0.0, axis_endpoint=None, layer="0", color=None)` | `center: tuple \| list`<br>`base_radius: float`<br>`height: float \| None`<br>`sides: int`<br>`axis_endpoint: tuple \| list \| None`<br>`layer: str` | `int` (handle) | Creates a regular $N$-sided pyramid with optional axis alignment toward `axis_endpoint`. |
| `solid_union(first, second, keep_operands=False)` | `first, second: int`<br>`keep_operands: bool` | `int` (handle) | Performs a boolean union of two solid handles. |
| `solid_subtract(first, second, keep_operands=False)` | `first, second: int`<br>`keep_operands: bool` | `int` (handle) | Performs a boolean subtraction (`first` - `second`). |
| `solid_intersect(first, second, keep_operands=False)` | `first, second: int`<br>`keep_operands: bool` | `int` (handle) | Performs a boolean intersection of two solid handles. |

---

## 5. Entity Query & Management

| Method | Arguments | Returns | Description |
| :--- | :--- | :--- | :--- |
| `remove_entity(handle)` | `handle: int` | `bool` | Deletes the entity with the specified handle. Returns `True` if found and deleted. |
| `entity_count()` | *None* | `int` | Returns the total count of entities in the active drawing. |
| `get_entities(kind=None, layer=None)` | `kind: str` *(optional)*<br>`layer: str` *(optional)* | `list[dict]` | Returns a list of entity dictionaries containing metadata such as `handle`, `kind`, `layer`. |
| `get_entity_data(handle)`<br>*(alias: `entget`)* | `handle: int` | `dict \| None` | Returns a dictionary containing the geometry and attributes of the entity (e.g. `start`/`end` for `LINE`, `center`/`radius`/`start_angle`/`end_angle` for `ARC`, `center`/`radius` for `CIRCLE`, `location` for `POINT`). Equivalent to OpenCAD LISP's `(entget ent)`. |

---

## 6. Drawing Tables, Styles & Blocks

| Method | Arguments | Returns | Description |
| :--- | :--- | :--- | :--- |
| `add_layer(name, ...)` | `name: str`<br>`color=None`<br>`linetype=None`<br>`lineweight=None`<br>`off=None`<br>`visible=None`<br>`on=None`<br>`frozen=None`<br>`locked=None`<br>`plottable=None`<br>`plot=None`<br>`transparency=None`<br>`description=None` | `int` | Creates a new layer via API v7 `TableOperation::LayerCreate`. Returns layer handle. |
| `modify_layer(name, ...)`<br>*(alias: `set_layer`)* | `name: str`<br>`...` | `bool` | Modifies an existing layer in-place via API v7 `TableOperation::LayerModify`. |
| `rename_layer(old_name, new_name)` | `old_name, new_name: str` | `bool` | Renames a layer. Entities on it automatically follow. |
| `delete_layer(name, erase_objects=False)` | `name: str`<br>`erase_objects: bool` | `bool` | Deletes a layer. Unused layers delete safely; set `erase_objects=True` to purge contents. |
| `set_layer_color(name, color)` | `name: str`<br>`color: int \| str \| tuple` | `bool` | Updates the color of an existing layer. |
| `get_layers()` | *None* | `list[dict]` | Returns a list of all layers with their properties. |
| `get_layer(name)` | `name: str` | `dict \| None` | Returns full property dictionary for a single layer by name. |
| `get_current_layer()` | *None* | `str` | Returns the name of the active drawing layer. |
| `set_current_layer(name)` | `name: str` | `bool` | Sets the active drawing layer by name. |
| `add_text_style(name, font_family=None, height=None, width_factor=None, oblique_angle=None)` | `name: str`<br>`font_family: str \| None`<br>`height, width_factor, oblique_angle: float \| None` | `int` | Creates a new text style via `TableOperation::TextStyleCreate`. |
| `define_block(name, base_point, entity_handles, erase_originals=False, description=None)` | `name: str`<br>`base_point: tuple \| list`<br>`entity_handles: list[int]`<br>`erase_originals: bool`<br>`description: str \| None` | `int` | Defines a reusable CAD block from existing entities. |
| `insert_block(name, pt, scale=None, rotation=0.0, layer="0", color=None)` | `name: str`<br>`pt: tuple \| list`<br>`scale: float \| tuple`<br>`rotation: float`<br>`layer: str` | `int` | Inserts an instance of a block definition at `pt`. |

---

## 7. System Variables & Selection

| Method | Arguments | Returns | Description |
| :--- | :--- | :--- | :--- |
| `get_var(name)` | `name: str` | `str \| float \| None` | Reads any host system variable (e.g. `CLAYER`, `LTSCALE`, `PDMODE`). |
| `set_var(name, value)` | `name: str`<br>`value: str \| float \| int` | `bool` | Sets a host system variable without re-entering command dispatch. |
| `set_selection(handles)` | `handles: list[int]` | `None` | Replaces active viewport selection with exactly these entity handles. |

---

## 6. CAD Commands, Decorators & Interop

| Method / Decorator | Arguments | Returns | Description |
| :--- | :--- | :--- | :--- |
| `@command`<br>*(or `@command("ALIAS")`)* | `fn` or `name: str` *(optional)* | `function` | Decorator to expose any Python function as a top-level CAD command (OpenCAD LISP-style). Functions remain directly callable/importable by other scripts. |
| `cmd(cmd_str)`<br>*(or `execute_command`)* | `cmd_str: str` | `bool` | Executes an OpenCAD Studio command string (e.g. `oc.cmd("ZOOM E")`, `oc.cmd("LINE 0,0 PAUSE ENTER")`). Supports CAD-style `PAUSE` (`\`), `ENTER` token, trailing `\n`, and streaming commands across lines. |
| `enter()` | *None* | `bool` | Sends an Enter keypress to advance or terminate the currently active CAD command (shortcut for `oc.cmd("ENTER")`). |
| `register_command(name, target)`<br>*(alias: `add_alias`)* | `name: str`<br>`target: str` | `bool` | Registers a top-level command alias pointing to another command or script (e.g. `oc.register_command("DG", "PY draw_grid")`). |
| `run_script(script_name, *args)` | `script_name: str`<br>`*args: str` *(optional)* | *None* | Executes another Python script located in the user scripts folder with arguments. |

---

## 7. Logging, Console & State

| Method | Arguments | Returns | Description |
| :--- | :--- | :--- | :--- |
| `info(msg)` | `msg: str` | *None* | Prints an informational message to the CAD command line (`i  <msg>`). |
| `output(msg)` | `msg: str` | *None* | Prints standard output to the CAD command line. |
| `error(msg)` | `msg: str` | *None* | Prints an error message to the CAD command line. |
| `undo(label)` | `label: str` | *None* | Records an undo boundary labeled with `label`. |
| `set_dirty()` | *None* | *None* | Marks the active drawing document as modified. |
| `bump_geometry()` | *None* | *None* | Forces the active viewport to re-tessellate geometry. |

---

## 8. Security & Sandboxing

The `opencad-py-runner` plugin includes a cross-platform (Windows, Linux, macOS) security sandbox designed to protect against malicious or accidental operations (blocking unauthorized network sockets, subprocess execution, native shared library loading, and out-of-boundary file system mutations).

### Python API
| Method | Arguments | Returns | Description |
| :--- | :--- | :--- | :--- |
| `is_sandboxed()` | *None* | `bool` | Returns `True` if the Python execution sandbox is currently active. |
| `get_security_status()` | *None* | `dict` | Returns dictionary containing current sandbox policy and trusted folder list (`sandbox_enabled`, `allow_network`, `allow_subprocesses`, `allow_native_binaries`, `max_entities_per_run`, `trusted_directories`). |
| `trust_directory(path)` | `path: str` | `bool` | Adds a directory path to the persisted trusted directories list. Returns `True` on success. |

### CAD Console Security Commands
- **`PYSECURITY`** (or `PYTRUST_LIST`): Displays the active security policy, flags, and all trusted directories in the OpenCAD command line.
- **`PYTRUST <folder_path>`**: Adds a folder to the persistent trusted directories list in `security_config.json`.
- **`PYUNTRUST <folder_path>`**: Removes a folder from the trusted directories list.

---

## 9. Complete Script Examples

### Example 1: Interactive Line Tool (`c-draw_line.py`)
Save as `~/.config/OpenCADStudio/scripts/c-draw_line.py` to automatically register the `DRAW_LINE` command:
```python
import opencad as oc

def run(args):
    # Collect two points interactively
    p1 = oc.get_point("Specify start point: ")
    if not p1:
        oc.info("Cancelled.")
        return

    p2 = oc.get_point("Specify end point: ")
    if not p2:
        oc.info("Cancelled.")
        return

    # Draw line on a dedicated layer with custom color
    h = oc.add_line(p1, p2, layer="MY_LINES", color="cyan")
    oc.info(f"Line created with handle {hex(h)} from {p1} to {p2}")

if __name__ == "__main__":
    run([])
```

### Example 2: Entity Inspector & Multi-Selection
```python
import opencad as oc

def run(args):
    # Prompt user to select multiple entities
    selection = oc.get_selection("Select objects to count: ")
    if not selection:
        oc.info("No objects selected.")
        return

    oc.info(f"Total objects selected: {len(selection)}")

    # Prompt user to pick a specific reference point
    pt = oc.get_point("Pick location for summary label: ")
    if pt:
        oc.add_text(f"Selected: {len(selection)} items", pt, height=5.0, color="yellow")

if __name__ == "__main__":
    run([])
```

### Example 3: OpenCAD LISP-Style Module with Helpers & `@oc.command`s (`tools_demo.py`)
Save as `~/.config/OpenCADStudio/scripts/tools_demo.py`:
```python
import opencad as oc

# --- Reusable Helper Functions (available for import anywhere) ---
def midpoint(p1, p2):
    return ((p1[0] + p2[0]) / 2.0, (p1[1] + p2[1]) / 2.0, (p1[2] + p2[2]) / 2.0)

def distance(p1, p2):
    return sum((b - a) ** 2 for a, b in zip(p1, p2)) ** 0.5

# --- Top-Level CAD Commands (decorated with @oc.command) ---
@oc.command
def my_box(width: float = 50.0, height: float = 50.0):
    """Draw a box from origin. Run in CAD console as: MY_BOX 100 80"""
    w, h = float(width), float(height)
    oc.add_line((0.0, 0.0), (w, 0.0), layer="BOXES")
    oc.add_line((w, 0.0), (w, h), layer="BOXES")
    oc.add_line((w, h), (0.0, h), layer="BOXES")
    oc.add_line((0.0, h), (0.0, 0.0), layer="BOXES")
    oc.info(f"Created box {w}x{h}")
    return w * h

@oc.command("TRIANGLE")
def draw_triangle(base: float = 30.0, height: float = 40.0):
    """Draw a triangle. Run in CAD console as: TRIANGLE 60 80"""
    oc.add_line((0.0, 0.0), (base, 0.0), layer="TRIANGLES")
    oc.add_line((base, 0.0), (0.0, height), layer="TRIANGLES")
    oc.add_line((0.0, height), (0.0, 0.0), layer="TRIANGLES")

@oc.command
def quick_line():
    """Interactive line that marks its midpoint. Run in CAD console as: QUICK_LINE"""
    p1 = oc.get_point("First point: ")
    p2 = oc.get_point("Second point: ")
    if p1 and p2:
        oc.add_line(p1, p2)
        oc.add_point(midpoint(p1, p2), layer="MIDPOINTS")
```

Other scripts can import both helpers and commands:
```python
from tools_demo import midpoint, distance, my_box

mid = midpoint((0, 0, 0), (10, 20, 0))
area = my_box(100, 50)  # Call programmatically as a regular function!
```
