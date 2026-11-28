### OpenCAD LISP & IDE — Español (es-ES)
### Mantenga los nombres de comandos CAD (OCIDE, APPLOAD, LSPLIST, etc.),
### símbolos LISP y marcadores { $variable } sin traducir.

## Language Selector
language-auto = Seguir a OpenCAD Studio / Sistema

## Ribbon
ribbon-tab-title = OpenCAD LISP
ribbon-group-scripting = Scripts OpenCAD LISP
ribbon-tool-ide = OpenCAD LISP IDE
ribbon-tool-appload = Cargar app
ribbon-tool-startup = Inicio
ribbon-tool-scripts = Scripts
ribbon-tool-reload = Recargar
ribbon-tool-folder = Carpeta

## Plugin Command-Line Messages
cmd-loaded = OpenCAD LISP cargado ({ $scripts } scripts .lsp en { $dir }, { $commands } comandos c: registrados)
cmd-reloaded = Motor OpenCAD LISP recargado: { $scripts } scripts .lsp en { $dir }, { $commands } nuevos comandos c: registrados
cmd-reload-failed = Error al recargar la carpeta de scripts: { $error }
cmd-scripts-dir = Carpeta de scripts: { $dir }
cmd-opening-ide = Abriendo OpenCAD LISP IDE (OCIDE)...
cmd-usage-lsprun = Uso: LSPRUN <código_lisp> o (command ...)
cmd-usage-lsp = Uso: LSP <nombre_o_archivo> o APPLOAD [<ruta>]
cmd-registered-commands = Comandos OpenCAD LISP registrados ({ $count }):
cmd-no-scripts-found = No se encontraron archivos OpenCAD LISP (.lsp) en { $dir }
cmd-available-scripts = Archivos OpenCAD LISP (.lsp) disponibles ({ $count }):
cmd-no-errors = No hay errores de OpenCAD LISP registrados en esta sesión.
cmd-last-error-title = Último error de OpenCAD LISP
cmd-python-shelved = El soporte de Python está desactivado actualmente. Use OpenCAD LISP (LSP, APPLOAD, LSPRUN o expresiones S directas).

## APPLOAD & STARTUP Dialogs
appload-title = Cargar/Descargar aplicaciones (APPLOAD)
appload-startup-title = Scripts de inicio
appload-btn-ok = Aceptar
appload-btn-cancel = Cancelar
appload-filter-lisp = Archivos OpenCAD LISP (*.lsp;*.lisp)
appload-section-load = Cargar en el dibujo actual
appload-label-load-file = Archivo OpenCAD LISP a cargar:
appload-checkbox-add-startup = Añadir también este archivo a Scripts de inicio (carga automática en cada dibujo)
appload-section-startup = Scripts de inicio (cargados automáticamente en cada dibujo nuevo)
appload-label-pick-startup = Seleccione un script para añadir a Scripts de inicio:
appload-info-auto-add = Los scripts seleccionados arriba se añaden automáticamente a la lista inferior.
appload-table-startup-only = Lista de scripts de inicio (carga automática en cada dibujo):
appload-table-remembered = Scripts de inicio guardados:
appload-col-path = Ruta de archivo autocargado
appload-status-added = Se añadió "{ $file }" a los scripts de inicio.
appload-status-removed = Se eliminó "{ $file }" de los scripts de inicio.
appload-err-not-found = APPLOAD: Script de inicio no encontrado: { $path }
appload-err-load-failed = Error al cargar OpenCAD LISP en { $path }: { $error }
appload-err-save-failed = APPLOAD: Error al guardar Scripts de inicio: { $error }
appload-info-updated = { $count ->
    [one] APPLOAD: Scripts de inicio actualizados ({ $count } archivo guardado).
   *[other] APPLOAD: Scripts de inicio actualizados ({ $count } archivos guardados).
}

## OCIDE Window & Welcome
ide-window-title = OpenCAD LISP IDE (OCIDE) - { $tab }
ide-welcome = === OpenCAD LISP IDE (OCIDE) ===
    Escriba código OpenCAD LISP arriba y pulse 'Cargar script' (F5) o 'Cargar selección' (F8).
    Use la línea REPL inferior para evaluación interactiva de OpenCAD LISP.

## OCIDE Toolbar
toolbar-continue-tip = Continuar (F5)
toolbar-step-over-tip = Pasar por encima (F10)
toolbar-step-into-tip = Entrar en (F11)
toolbar-step-out-tip = Salir de (Shift+F11)
toolbar-restart-tip = Reiniciar (Ctrl+Shift+F5)
toolbar-restart-rerun-tip = Reiniciar / Volver a ejecutar (Ctrl+Shift+F5)
toolbar-stop-tip = Detener (Shift+F5)
toolbar-debugging-badge = ● DEPURANDO
toolbar-load-script-tip = Cargar script (F5)
toolbar-load-selection-tip = Cargar selección (F8)
toolbar-start-debugging-tip = Iniciar depuración
toolbar-new-tab-tip = Nueva pestaña de script
toolbar-open = Abrir
toolbar-open-tip = Abrir archivo de script
toolbar-save = Guardar
toolbar-save-tip = Guardar archivo de script (Ctrl+S)
toolbar-save-as = Guardar como...
toolbar-save-as-tip = Guardar script como nuevo archivo (Ctrl+Shift+S)
toolbar-undo-tip = Deshacer (Ctrl+Z)
toolbar-redo-tip = Rehacer (Ctrl+Y / Ctrl+Shift+Z)
toolbar-format = Formatear
toolbar-format-tip = Formatear documento OpenCAD LISP (Shift+Alt+F) | Selección (Ctrl+Shift+F)
toolbar-find = Buscar
toolbar-find-tip = Buscar (Ctrl+F) | Reemplazar (Ctrl+H)
toolbar-variables = Variables
toolbar-variables-tip = Mostrar/Ocultar panel de variables
toolbar-function-list = λ Funciones
toolbar-function-list-tip = Mostrar/Ocultar lista de funciones y referencia OpenCAD LISP (F2)
toolbar-clear-output = Limpiar salida
toolbar-clear-output-tip = Limpiar consola de salida
toolbar-switch-to-cad = Ir a CAD
toolbar-transparency-tip = Alternar transparencia de ventana
toolbar-settings = Ajustes
toolbar-settings-tip = Abrir ajustes del IDE
toolbar-help = Ayuda ▾
toolbar-help-tip = Ayuda y tutoriales
toolbar-tutorials = Tutoriales

## Editor, Context Menu & Autocomplete
editor-no-open-file = Ningún archivo abierto
editor-no-active-file = Ningún archivo activo
ctx-copy = Copiar (Ctrl+C)
ctx-paste = Pegar (Ctrl+V)
ctx-find = Buscar (Ctrl+F)
ctx-replace = Reemplazar (Ctrl+H)
ctx-format-selection = Formatear selección (Ctrl+Shift+F)
ctx-format-document = Formatear documento (Shift+Alt+F)
ctx-add-watch = Añadir a inspección (Shift+F9)
ctx-add-watch-ident = Añadir a inspección: '{ $ident }' (Shift+F9)
ctx-toggle-bp-preview = Alternar punto de ruptura: { $preview } (F9)
ctx-toggle-bp-line = Alternar punto de ruptura (Línea { $line }, F9)
autocomplete-custom = Personalizado
autocomplete-navigate = ↑/↓ Navegar
autocomplete-complete = Tab Completar

## Find & Replace Overlay
find-placeholder = Buscar...
find-replace-placeholder = Reemplazar por...
find-no-results = Sin resultados
find-match-of = { $current } de { $total }
find-match-count = { $count ->
    [one] { $count } coincidencia
   *[other] { $count } coincidencias
}
find-btn-replace = Reemplazar
find-btn-all = Todo

## Status Bar
status-paused-span = ▶ EN PAUSA en Lín { $line }, Col { $col } { $expr }  |  F5: Continuar  |  F10: Pasar  |  F11: Entrar  |  Shift+F11: Salir  |  Shift+F5: Detener  |  { $count ->
    [one] { $count } punto de ruptura
   *[other] { $count } puntos de ruptura
}
status-paused-line = ▶ EN PAUSA en Línea { $line }  |  F5: Continuar  |  F10: Pasar  |  F11: Entrar  |  Shift+F11: Salir  |  Shift+F5: Detener  |  { $count ->
    [one] { $count } punto de ruptura
   *[other] { $count } puntos de ruptura
}
status-debugging-active = 🐞 DEPURACIÓN activa  |  { $count ->
    [one] { $count } punto de ruptura
   *[other] { $count } puntos de ruptura
} (F9: Alternar PR)
status-breakpoints-list = Puntos de ruptura: [{ $list }]
status-no-breakpoints = Sin puntos de ruptura
status-ready = Lín { $line }, Col { $col }  |  { $lines ->
    [one] { $lines } línea
   *[other] { $lines } líneas
}  |  { $bps }  |  OpenCAD LISP  |  F5: Ejecutar, F9: Punto de ruptura, Shift+Alt+F: Formatear

## Console & REPL
console-title = Consola / Salida
console-repl-hint = REPL interactivo OpenCAD LISP (Enter para ejecutar, Ctrl+Enter para nueva línea)
repl-placeholder-lisp = Código / expresión OpenCAD LISP (p. ej. (+ 1 2) o (getpoint) - Enter para ejecutar, Ctrl+Enter nueva línea)...
repl-placeholder-python = Código / expresión Python (Enter para ejecutar, Ctrl+Enter para nueva línea)...
repl-btn-run = Ejecutar
repl-btn-run-lisp-tip = Ejecutar código OpenCAD LISP (Enter para ejecutar, Ctrl+Enter nueva línea)
repl-btn-run-python-tip = Ejecutar código Python (Enter para ejecutar, Ctrl+Enter nueva línea)
repl-btn-clear = Limpiar
repl-btn-clear-tip = Limpiar entrada REPL

## Debug Inspector Drawer
debug-inspector-title = Inspector de depuración
debug-watch-title = Expresiones de inspección
debug-col-expression = Expresión
debug-col-value = Valor
debug-btn-clear = Limpiar
debug-watch-empty = Sin expresiones de inspección (Clic derecho o Shift+F9 sobre variable)
debug-not-in-scope = <fuera de ámbito>
debug-not-debugging = <sin depurar>
debug-locals-title = Variables locales
debug-col-name = Nombre
debug-col-type = Tipo
debug-locals-empty-debugging = Sin variables locales en el marco
debug-locals-empty-idle = Inicie la depuración para inspeccionar variables
debug-stack-title = Pila de llamadas
debug-col-function = Función
debug-col-line = Línea
debug-col-file = Archivo
debug-stack-empty-debugging = Pila de llamadas vacía
debug-stack-empty-idle = Sin pila de llamadas activa
debug-breakpoints-title = Puntos de ruptura
debug-breakpoints-empty = Sin puntos de ruptura (Pulse F9 en '(' o expresión S)

## API Reference Drawer
api-drawer-title = λ Lista de funciones y referencia OpenCAD LISP
api-search-placeholder = Buscar API OpenCAD LISP y DXF...
api-btn-insert = + Insertar
api-active-prefix = λ Activa: { $name }
api-active-none = Expresión activa: (Ninguna en el cursor)

## Settings Dialog
settings-title = ⚙ Ajustes del IDE
settings-btn-done = Listo
settings-tab-general = ⚙ General y Depurador
settings-tab-editor = 🎨 Editor, Fuente y Sintaxis
settings-tab-formatting = 📐 Formato OpenCAD LISP
settings-language-title = Idioma de la interfaz
settings-language-label = Idioma:
settings-language-note = Los cambios se aplican inmediatamente en OCIDE. Reinicie OpenCAD Studio para actualizar la cinta.
settings-bp-color-title = Color de línea de punto de ruptura
settings-exec-color-title = Color de línea de ejecución actual
settings-transparency-label = Nivel de transparencia:
settings-debugger-automation-title = Automatización del depurador
settings-auto-transparency = Cambiar automáticamente el IDE a transparente al depurar (y restaurar al terminar)
settings-scripts-dir-title = Ubicación predeterminada de scripts
settings-btn-browse = 📂 Examinar...
settings-btn-reset = ↺ Restablecer
settings-editor-font-label = Fuente del editor:
settings-font-size-label = Tamaño:
settings-btn-default-font = ↺ Fuente predeterminada
settings-theme-label = Tema:
settings-theme-new-placeholder = Nombre del nuevo tema...
settings-btn-save = 💾 Guardar
settings-btn-reset-defaults = ↺ Restablecer todo
settings-syntax-elements-title = Elementos de sintaxis
settings-syntax-bg = Fondo del editor
settings-syntax-default-text = Texto predeterminado
settings-syntax-comments = Comentarios (;)
settings-syntax-comments-short = Comentarios
settings-syntax-strings = Cadenas ("...")
settings-syntax-strings-short = Cadenas
settings-syntax-keywords = Palabras clave y formas
settings-syntax-native-funcs = Funciones nativas
settings-syntax-custom-funcs = Funciones personalizadas
settings-syntax-user-custom-funcs = Funciones personalizadas del usuario
settings-syntax-numbers = Números
settings-syntax-parens = Paréntesis ()
settings-syntax-parens-short = Paréntesis
settings-adjust-color = Ajustar color: { $target }
settings-hex-label = Hex:
settings-btn-apply = Aplicar
settings-quick-palette = Paleta rápida:
settings-custom-funcs-empty = Aún no se han añadido funciones personalizadas.
settings-custom-funcs-placeholder = Nombre de nueva función...
settings-btn-add = + Añadir
settings-btn-remove = − Eliminar
settings-custom-funcs-selected = Seleccionada: { $name }
settings-custom-funcs-select-hint = Seleccione un elemento de la lista para eliminar
settings-color-mode-label = Modo de color:
settings-color-mode-native = Igual que nativas
settings-color-mode-dedicated = Color personalizado dedicado
settings-manage-func-names = Administrar nombres de funciones:
settings-live-preview-title = Vista previa de sintaxis en vivo
settings-formatting-title = Reglas de formato OpenCAD LISP
settings-formatting-subtitle = Configure reglas de sangría automática, mayúsculas/minúsculas de símbolos y alineación según LISP clásico y especificaciones CAD estándar.
settings-indent-label = Tamaño de sangría:
settings-indent-2 = 2 espacios (predeterminado)
settings-indent-3 = 3 espacios
settings-indent-4 = 4 espacios
settings-reflow-label = Redistribución estructural: mover expresiones entre líneas (saltos y uniones LISP clásicas)
settings-reflow-note = Expande cuerpos defun, progn, bucles while/repeat, setq multipar y cláusulas cond en líneas separadas; compacta átomos y paréntesis fragmentados en una sola línea.
settings-downcase-label = Convertir palabras clave y funciones integradas a minúsculas (defun, setq, getpoint, princ, etc.)
settings-downcase-note = Estandariza palabras clave y funciones integradas a minúsculas, y constantes booleanas a T y nil, preservando las mayúsculas/minúsculas originales de variables y cadenas del usuario.
settings-wide-label = Estilo ancho: alinear argumentos multilínea bajo el primer argumento
settings-wide-note = Para llamadas normales como (command "_.LINE" ...) o (strcat ...), las líneas siguientes se alinean verticalmente bajo el primer argumento de la línea 1.
settings-align-setq-label = Alinear pares variable-valor en bloques (setq ...)
settings-align-setq-note = En asignaciones (setq ...) multilínea, los nombres de variables siguientes se alinean verticalmente bajo la primera variable.
settings-shortcuts-title = Atajos de teclado y menú contextual:
settings-shortcut-format-doc-label = • Formatear documento completo:
settings-shortcut-format-doc-keys = Shift+Alt+F  o  Ctrl+Alt+F (predeterminado)
settings-shortcut-format-sel-label = • Formatear selección:
settings-shortcut-format-sel-keys = Ctrl+Shift+F  o  Clic derecho → Formatear selección
settings-shortcut-smart-enter-label = • Sangría inteligente con Enter:
settings-shortcut-smart-enter-desc = Enter calcula automáticamente la sangría según las reglas actuales
settings-shortcut-comments-label = • Comentarios:
settings-shortcut-comments-desc = ;;; permanece en la columna 0  |  ;; se alinea al bloque de código
settings-btn-format-now = ⚡ Formatear documento activo ahora

## Save Confirmation Dialog
save-dialog-title = 💾 Cambios sin guardar
save-dialog-prompt-single = ¿Desea guardar los cambios en "{ $file }"?
save-dialog-prompt-multi = ¿Desea guardar los cambios en { $count } archivos antes de cerrar?
save-dialog-warning = Sus cambios se perderán si no los guarda.
save-dialog-btn-save = Guardar
save-dialog-btn-dont-save = No guardar
save-dialog-btn-cancel = Cancelar

## Find & Replace Tooltips
find-toggle-replace-tip = Alternar reemplazo
find-prev-tip = Coincidencia anterior (Shift+F3)
find-next-tip = Siguiente coincidencia (F3)
find-case-tip = Coincidir mayúsculas y minúsculas
find-word-tip = Coincidir palabra completa
find-close-tip = Cerrar (Esc)

## OCIDE File Dialog Filters & Console Status Messages
dialog-filter-lisp = Rutina OpenCAD LISP (*.lsp, *.lisp)
dialog-filter-all = Todos los archivos (*.*)
console-debug-paused = [DEPURACIÓN] Pausado en { $file }:{ $line }:{ $col }
console-debug-paused-expr = [DEPURACIÓN] Pausado en { $file }:{ $line }:{ $col } -> { $expr }
console-debug-finished = [DEPURACIÓN] Ejecución finalizada.
console-debug-error = [ERROR DE DEPURACIÓN] { $error }
console-cad-selection = { $count ->
    [one] [Selección CAD] { $count } entidad seleccionada: { $handles }
   *[other] [Selección CAD] { $count } entidades seleccionadas: { $handles }
}
console-ide-opened = [IDE] Abierto { $path }
console-ide-saved = [IDE] Guardado { $file }
console-repl-mode-switched = [Modo REPL cambiado a { $mode }]
console-cleared = === Consola limpiada ===
console-loading-script = --- Cargando script: { $file } ---
console-tutorials-open-failed = No se pudieron abrir los tutoriales ({ $path }): { $error }
console-tutorials-not-found = No se encontró el archivo index.html de tutoriales.

## Settings Color Swatch Tooltips
color-dark-red-default = Rojo oscuro (predeterminado)
color-crimson = Carmesí
color-burgundy = Borgoña
color-purple = Púrpura
color-orange = Naranja
color-navy-blue = Azul marino
color-charcoal = Carbón
color-teal = Verde azulado
color-green-default = Verde (predeterminado)
color-emerald = Esmeralda
color-lime = Lima
color-yellow-gold = Amarillo / Dorado
color-cyan = Cian
color-sky-blue = Azul cielo
color-mocha-default = Predeterminado Base16 Mocha
color-pure-white = Blanco puro (#FFFFFF)
color-soft-cream = Crema suave (#F8F8F2)
color-light-gray = Gris claro (#D0C8C6)
color-muted-gray = Gris tenue (#839496)
color-charcoal-hex = Carbón (#272822)
color-mocha-bg = Fondo Mocha (#202225)
color-pure-black = Negro puro (#000000)
color-solarized-dark = Solarized oscuro (#002B36)
color-red-coral = Rojo / Coral (#FF5555)
color-orange-hex = Naranja (#FFB86C)
color-gold-yellow = Dorado / Amarillo (#F1FA8C)
color-emerald-green = Verde esmeralda (#50FA7B)
color-cyan-teal = Cian / Verde azulado (#8BE9FD)
color-classic-blue = Azul clásico (#0000FF)
color-navy-hex = Azul marino (#000080)
color-sky-blue-hex = Azul cielo (#82AAFF)
color-purple-lavender = Púrpura / Lavanda (#BD93F9)
color-magenta = Magenta (#FF79C6)
color-classic-purple = Púrpura clásico (#800080)
color-sienna-brown = Marrón siena (#A0522D)
