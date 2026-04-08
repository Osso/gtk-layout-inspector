//! Layout dump output format.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Truncate a string to max_chars, adding "..." if truncated.
/// Handles multi-byte UTF-8 characters correctly.
fn truncate_str(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars - 3).collect();
        format!("{}...", truncated)
    }
}

/// Information about a widget's type and content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WidgetInfo {
    Window {
        title: Option<String>,
    },
    Box {
        orientation: String,
    },
    Button {
        label: Option<String>,
    },
    Label {
        text: String,
    },
    Entry {
        text: String,
        placeholder: Option<String>,
    },
    TextView {
        text: String,
    },
    ScrolledWindow,
    ListBox,
    ListBoxRow,
    Stack,
    StackPage {
        name: Option<String>,
    },
    HeaderBar {
        title: Option<String>,
    },
    Paned {
        orientation: String,
    },
    Notebook,
    Grid,
    FlowBox,
    Picture,
    Image,
    Spinner {
        spinning: bool,
    },
    ProgressBar {
        fraction: f64,
    },
    Scale {
        value: f64,
    },
    Switch {
        active: bool,
    },
    CheckButton {
        active: bool,
        label: Option<String>,
    },
    ToggleButton {
        active: bool,
        label: Option<String>,
    },
    ComboBox,
    DropDown,
    Popover,
    MenuButton {
        label: Option<String>,
    },
    Revealer {
        revealed: bool,
    },
    Expander {
        expanded: bool,
        label: Option<String>,
    },
    Separator,
    Frame {
        label: Option<String>,
    },
    AspectFrame,
    Overlay,
    Fixed,
    DrawingArea,
    GLArea,
    Video,
    MediaControls,
    Calendar,
    ColorButton,
    FontButton,
    LinkButton {
        uri: String,
        label: Option<String>,
    },
    LevelBar {
        value: f64,
    },
    SearchEntry {
        text: String,
    },
    PasswordEntry,
    SpinButton {
        value: f64,
    },
    /// Unknown widget type - includes the GTK type name
    Unknown {
        type_name: String,
    },
}

impl WidgetInfo {
    /// Get a short description for display.
    pub fn short_desc(&self) -> String {
        match self {
            Self::Window { title } => title
                .as_ref()
                .map_or("Window".into(), |t| format!("Window \"{}\"", t)),
            Self::Box { orientation } => format!("Box({})", orientation),
            Self::Button { label } => label
                .as_ref()
                .map_or("Button".into(), |l| format!("Button \"{}\"", l)),
            Self::Label { text } => {
                let truncated = truncate_str(text, 60);
                format!("Label \"{}\"", truncated)
            }
            Self::Entry { placeholder, .. } => placeholder
                .as_ref()
                .map_or("Entry".into(), |p| format!("Entry [{}]", p)),
            Self::TextView { .. } => "TextView".into(),
            Self::ScrolledWindow => "ScrolledWindow".into(),
            Self::ListBox => "ListBox".into(),
            Self::ListBoxRow => "ListBoxRow".into(),
            Self::Stack => "Stack".into(),
            Self::StackPage { name } => name
                .as_ref()
                .map_or("StackPage".into(), |n| format!("StackPage \"{}\"", n)),
            Self::HeaderBar { title } => title
                .as_ref()
                .map_or("HeaderBar".into(), |t| format!("HeaderBar \"{}\"", t)),
            Self::Paned { orientation } => format!("Paned({})", orientation),
            Self::Notebook => "Notebook".into(),
            Self::Grid => "Grid".into(),
            Self::FlowBox => "FlowBox".into(),
            Self::Picture => "Picture".into(),
            Self::Image => "Image".into(),
            Self::Spinner { spinning } => {
                format!("Spinner({})", if *spinning { "on" } else { "off" })
            }
            Self::ProgressBar { fraction } => format!("ProgressBar({:.0}%)", fraction * 100.0),
            Self::Scale { value } => format!("Scale({:.1})", value),
            Self::Switch { active } => format!("Switch({})", if *active { "on" } else { "off" }),
            Self::CheckButton { active, label } => {
                let state = if *active { "[x]" } else { "[ ]" };
                label
                    .as_ref()
                    .map_or(format!("CheckButton {}", state), |l| {
                        format!("CheckButton {} \"{}\"", state, l)
                    })
            }
            Self::ToggleButton { active, label } => {
                let state = if *active { "on" } else { "off" };
                label
                    .as_ref()
                    .map_or(format!("ToggleButton({})", state), |l| {
                        format!("ToggleButton({}) \"{}\"", state, l)
                    })
            }
            Self::ComboBox => "ComboBox".into(),
            Self::DropDown => "DropDown".into(),
            Self::Popover => "Popover".into(),
            Self::MenuButton { label } => label
                .as_ref()
                .map_or("MenuButton".into(), |l| format!("MenuButton \"{}\"", l)),
            Self::Revealer { revealed } => {
                format!("Revealer({})", if *revealed { "shown" } else { "hidden" })
            }
            Self::Expander { expanded, label } => {
                let state = if *expanded { "open" } else { "closed" };
                label.as_ref().map_or(format!("Expander({})", state), |l| {
                    format!("Expander({}) \"{}\"", state, l)
                })
            }
            Self::Separator => "Separator".into(),
            Self::Frame { label } => label
                .as_ref()
                .map_or("Frame".into(), |l| format!("Frame \"{}\"", l)),
            Self::AspectFrame => "AspectFrame".into(),
            Self::Overlay => "Overlay".into(),
            Self::Fixed => "Fixed".into(),
            Self::DrawingArea => "DrawingArea".into(),
            Self::GLArea => "GLArea".into(),
            Self::Video => "Video".into(),
            Self::MediaControls => "MediaControls".into(),
            Self::Calendar => "Calendar".into(),
            Self::ColorButton => "ColorButton".into(),
            Self::FontButton => "FontButton".into(),
            Self::LinkButton { label, .. } => label
                .as_ref()
                .map_or("LinkButton".into(), |l| format!("LinkButton \"{}\"", l)),
            Self::LevelBar { value } => format!("LevelBar({:.0}%)", value * 100.0),
            Self::SearchEntry { .. } => "SearchEntry".into(),
            Self::PasswordEntry => "PasswordEntry".into(),
            Self::SpinButton { value } => format!("SpinButton({:.1})", value),
            Self::Unknown { type_name } => type_name.clone(),
        }
    }
}

/// A single entry in the layout dump.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutEntry {
    pub depth: usize,
    pub info: WidgetInfo,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub visible: bool,
    pub sensitive: bool,
    pub css_classes: Vec<String>,
    pub widget_name: Option<String>,
    /// Background color as hex (e.g., "#1e1e2e") if available
    pub background_color: Option<String>,
    /// Foreground/text color as hex (e.g., "#ffffff") if available
    pub foreground_color: Option<String>,
}

impl LayoutEntry {
    /// Format as a single line for text output.
    pub fn format_line(&self) -> String {
        let indent = "  ".repeat(self.depth);
        let flags = format!(
            "{}{}",
            if self.visible { "" } else { " [hidden]" },
            if self.sensitive { "" } else { " [insensitive]" },
        );
        let classes = if self.css_classes.is_empty() {
            String::new()
        } else {
            format!(" .{}", self.css_classes.join("."))
        };
        let name = self
            .widget_name
            .as_ref()
            .map_or(String::new(), |n| format!(" #{}", n));
        let bg = self
            .background_color
            .as_ref()
            .map_or(String::new(), |c| format!(" bg:{}", c));
        let fg = self
            .foreground_color
            .as_ref()
            .map_or(String::new(), |c| format!(" fg:{}", c));

        format!(
            "{}{} @ ({}, {}) {}x{}{}{}{}{}{}",
            indent,
            self.info.short_desc(),
            self.x,
            self.y,
            self.width,
            self.height,
            flags,
            classes,
            name,
            bg,
            fg,
        )
    }
}

/// Complete layout dump of a widget tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutDump {
    pub entries: Vec<LayoutEntry>,
}

impl LayoutDump {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add an entry to the dump.
    pub fn push(&mut self, entry: LayoutEntry) {
        self.entries.push(entry);
    }

    /// Get the number of widgets in the dump.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the dump is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Convert to JSON for structured processing.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Find entries matching a predicate.
    pub fn find<F>(&self, predicate: F) -> Vec<&LayoutEntry>
    where
        F: Fn(&LayoutEntry) -> bool,
    {
        self.entries.iter().filter(|e| predicate(e)).collect()
    }

    /// Find buttons by label text.
    pub fn find_buttons(&self, label: &str) -> Vec<&LayoutEntry> {
        self.find(|e| matches!(&e.info, WidgetInfo::Button { label: Some(l) } if l.contains(label)))
    }

    /// Find entries by placeholder text.
    pub fn find_entries(&self, placeholder: &str) -> Vec<&LayoutEntry> {
        self.find(|e| {
            matches!(&e.info, WidgetInfo::Entry { placeholder: Some(p), .. } if p.contains(placeholder))
        })
    }
}

impl Default for LayoutDump {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for LayoutDump {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "=== GTK Layout Dump ({} widgets) ===",
            self.entries.len()
        )?;
        for entry in &self.entries {
            writeln!(f, "{}", entry.format_line())?;
        }
        Ok(())
    }
}
