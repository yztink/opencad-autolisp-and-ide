use std::sync::OnceLock;
use ocs_plugin_api::ribbon::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

pub struct OpenCadLispRibbonModule;

impl CadModule for OpenCadLispRibbonModule {
    fn id(&self) -> &'static str {
        crate::MANIFEST.id
    }

    fn title(&self) -> &'static str {
        "OpenCAD LISP"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: OnceLock<Vec<RibbonGroup>> = OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![RibbonGroup {
                title: "OpenCAD LISP Scripting",
                tools: vec![
                    RibbonItem::LargeTool(ToolDef {
                        id: "OCIDE",
                        label: "OpenCAD LISP IDE",
                        icon: IconKind::Glyph("IDE"),
                        event: ModuleEvent::Command("OCIDE".to_string()),
                    }),
                    RibbonItem::LargeTool(ToolDef {
                        id: "APPLOAD",
                        label: "Appload",
                        icon: IconKind::Glyph("📥"),
                        event: ModuleEvent::Command("APPLOAD".to_string()),
                    }),
                    RibbonItem::LargeTool(ToolDef {
                        id: "STARTUP",
                        label: "Startup",
                        icon: IconKind::Glyph("⚡"),
                        event: ModuleEvent::Command("STARTUP".to_string()),
                    }),
                    RibbonItem::LargeTool(ToolDef {
                        id: "LSPLIST",
                        label: "Scripts",
                        icon: IconKind::Glyph("λ"),
                        event: ModuleEvent::Command("LSPLIST".to_string()),
                    }),
                    RibbonItem::LargeTool(ToolDef {
                        id: "LSPRELOAD",
                        label: "Reload",
                        icon: IconKind::Glyph("↻"),
                        event: ModuleEvent::Command("LSPRELOAD".to_string()),
                    }),
                    RibbonItem::LargeTool(ToolDef {
                        id: "LSPDIR",
                        label: "Folder",
                        icon: IconKind::Glyph("📁"),
                        event: ModuleEvent::Command("LSPDIR".to_string()),
                    }),
                ],
            }]
        })
    }
}
