use std::sync::OnceLock;
use ocs_plugin_api::ribbon::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

pub struct OpenCadLispRibbonModule;

impl CadModule for OpenCadLispRibbonModule {
    fn id(&self) -> &'static str {
        crate::MANIFEST.id
    }

    fn title(&self) -> &'static str {
        static TITLE: OnceLock<&'static str> = OnceLock::new();
        *TITLE.get_or_init(|| crate::i18n::tr_static("ribbon-tab-title"))
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: OnceLock<Vec<RibbonGroup>> = OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![RibbonGroup {
                title: crate::i18n::tr_static("ribbon-group-scripting"),
                tools: vec![
                    RibbonItem::LargeTool(ToolDef {
                        id: "OCIDE",
                        label: crate::i18n::tr_static("ribbon-tool-ide"),
                        icon: IconKind::Glyph("IDE"),
                        event: ModuleEvent::Command("OCIDE".to_string()),
                    }),
                    RibbonItem::LargeTool(ToolDef {
                        id: "APPLOAD",
                        label: crate::i18n::tr_static("ribbon-tool-appload"),
                        icon: IconKind::Glyph("📥"),
                        event: ModuleEvent::Command("APPLOAD".to_string()),
                    }),
                    RibbonItem::LargeTool(ToolDef {
                        id: "STARTUP",
                        label: crate::i18n::tr_static("ribbon-tool-startup"),
                        icon: IconKind::Glyph("⚡"),
                        event: ModuleEvent::Command("STARTUP".to_string()),
                    }),
                    RibbonItem::LargeTool(ToolDef {
                        id: "LSPLIST",
                        label: crate::i18n::tr_static("ribbon-tool-scripts"),
                        icon: IconKind::Glyph("λ"),
                        event: ModuleEvent::Command("LSPLIST".to_string()),
                    }),
                    RibbonItem::LargeTool(ToolDef {
                        id: "LSPRELOAD",
                        label: crate::i18n::tr_static("ribbon-tool-reload"),
                        icon: IconKind::Glyph("↻"),
                        event: ModuleEvent::Command("LSPRELOAD".to_string()),
                    }),
                    RibbonItem::LargeTool(ToolDef {
                        id: "LSPDIR",
                        label: crate::i18n::tr_static("ribbon-tool-folder"),
                        icon: IconKind::Glyph("📁"),
                        event: ModuleEvent::Command("LSPDIR".to_string()),
                    }),
                ],
            }]
        })
    }
}
