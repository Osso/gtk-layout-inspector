//! Widget tree traversal for GTK4.

use gtk4::prelude::*;
use gtk4::{self as gtk};
use std::panic;

use crate::output::{LayoutDump, LayoutEntry, WidgetInfo};

/// Dump the widget tree starting from the given widget.
pub fn dump_widget_tree(widget: &impl IsA<gtk::Widget>) -> LayoutDump {
    let mut dump = LayoutDump::new();
    // Catch panics during traversal to prevent app crash
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        traverse_widget(widget.as_ref(), 0, &mut dump);
    }));
    if let Err(e) = result {
        eprintln!("[gtk-debug] Panic during traversal: {:?}", e);
    }
    dump
}

/// Find a button by its label text.
pub fn find_button_by_label(widget: &impl IsA<gtk::Widget>, label: &str) -> Option<gtk::Button> {
    find_widget(widget.as_ref(), |w| {
        w.downcast_ref::<gtk::Button>()
            .and_then(|b| b.label())
            .is_some_and(|l| l.contains(label))
    })
    .and_then(|w| w.downcast::<gtk::Button>().ok())
}

/// Find an entry by its placeholder text.
pub fn find_entry_by_placeholder(
    widget: &impl IsA<gtk::Widget>,
    placeholder: &str,
) -> Option<gtk::Entry> {
    find_widget(widget.as_ref(), |w| {
        w.downcast_ref::<gtk::Entry>()
            .and_then(|e| e.placeholder_text())
            .is_some_and(|p| p.contains(placeholder))
    })
    .and_then(|w| w.downcast::<gtk::Entry>().ok())
}

/// Find a widget matching a predicate.
fn find_widget<F>(widget: &gtk::Widget, predicate: F) -> Option<gtk::Widget>
where
    F: Fn(&gtk::Widget) -> bool + Copy,
{
    if predicate(widget) {
        return Some(widget.clone());
    }

    let mut child = widget.first_child();
    while let Some(c) = child {
        if let Some(found) = find_widget(&c, predicate) {
            return Some(found);
        }
        child = c.next_sibling();
    }

    None
}

fn traverse_widget(widget: &gtk::Widget, depth: usize, dump: &mut LayoutDump) {
    let info = identify_widget(widget);
    let (x, y, width, height) = get_allocation(widget);

    let entry = LayoutEntry {
        depth,
        info,
        x,
        y,
        width,
        height,
        visible: widget.is_visible(),
        sensitive: widget.is_sensitive(),
        css_classes: widget.css_classes().iter().map(|s| s.to_string()).collect(),
        widget_name: {
            let name = widget.widget_name();
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        },
        background_color: get_background_color(widget),
        foreground_color: get_foreground_color(widget),
    };

    dump.push(entry);

    // Traverse children
    let mut child = widget.first_child();
    while let Some(c) = child {
        traverse_widget(&c, depth + 1, dump);
        child = c.next_sibling();
    }
}

fn get_allocation(widget: &gtk::Widget) -> (i32, i32, i32, i32) {
    let width = widget.width();
    let height = widget.height();

    // Get position relative to toplevel
    if let Some(toplevel) = widget.root() {
        let toplevel_widget: &gtk::Widget = toplevel.upcast_ref();
        if let Some((x, y)) = widget.translate_coordinates(toplevel_widget, 0.0, 0.0) {
            return (x as i32, y as i32, width, height);
        }
    }

    (0, 0, width, height)
}

/// Convert RGBA to hex color string.
fn rgba_to_hex(rgba: &gtk::gdk::RGBA) -> String {
    let r = (rgba.red() * 255.0) as u8;
    let g = (rgba.green() * 255.0) as u8;
    let b = (rgba.blue() * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

/// Try to get the background color from a widget's style context.
/// Falls back to foreground color if no background is found.
fn get_background_color(widget: &gtk::Widget) -> Option<String> {
    use gtk::prelude::StyleContextExt;

    let ctx = widget.style_context();

    // Try various color name patterns used by different apps/themes
    let color_names = [
        // libadwaita colors
        "window_bg_color",
        "view_bg_color",
        "card_bg_color",
        "headerbar_bg_color",
        "popover_bg_color",
        "dialog_bg_color",
        "sidebar_bg_color",
        "accent_bg_color",
        // Older GTK3-style names (some themes still use these)
        "theme_bg_color",
        "theme_base_color",
        "bg_color",
        "base_color",
        // Custom app colors
        "ayu_bg",
        "ayu_bg_card",
    ];

    for name in color_names {
        if let Some(color) = ctx.lookup_color(name) {
            return Some(format!("{}={}", name, rgba_to_hex(&color)));
        }
    }

    None
}

/// Get the foreground/text color from a widget's style context.
fn get_foreground_color(widget: &gtk::Widget) -> Option<String> {
    use gtk::prelude::StyleContextExt;

    let ctx = widget.style_context();
    let fg_color = ctx.color();
    Some(rgba_to_hex(&fg_color))
}

fn identify_widget(widget: &gtk::Widget) -> WidgetInfo {
    // Try each widget type in order of specificity

    // Windows
    if let Some(w) = widget.downcast_ref::<gtk::ApplicationWindow>() {
        return WidgetInfo::Window {
            title: w.title().map(|s| s.to_string()),
        };
    }
    if let Some(w) = widget.downcast_ref::<gtk::Window>() {
        return WidgetInfo::Window {
            title: w.title().map(|s| s.to_string()),
        };
    }

    // Containers
    if let Some(b) = widget.downcast_ref::<gtk::Box>() {
        let orientation = match b.orientation() {
            gtk::Orientation::Horizontal => "horizontal",
            gtk::Orientation::Vertical => "vertical",
            _ => "unknown",
        };
        return WidgetInfo::Box {
            orientation: orientation.to_string(),
        };
    }

    if widget.downcast_ref::<gtk::ScrolledWindow>().is_some() {
        return WidgetInfo::ScrolledWindow;
    }

    if widget.downcast_ref::<gtk::ListBox>().is_some() {
        return WidgetInfo::ListBox;
    }

    if widget.downcast_ref::<gtk::ListBoxRow>().is_some() {
        return WidgetInfo::ListBoxRow;
    }

    if widget.downcast_ref::<gtk::Stack>().is_some() {
        return WidgetInfo::Stack;
    }

    if let Some(hb) = widget.downcast_ref::<gtk::HeaderBar>() {
        return WidgetInfo::HeaderBar {
            title: hb
                .title_widget()
                .and_then(|w| w.downcast::<gtk::Label>().ok())
                .map(|l| l.text().to_string()),
        };
    }

    if let Some(p) = widget.downcast_ref::<gtk::Paned>() {
        let orientation = match p.orientation() {
            gtk::Orientation::Horizontal => "horizontal",
            gtk::Orientation::Vertical => "vertical",
            _ => "unknown",
        };
        return WidgetInfo::Paned {
            orientation: orientation.to_string(),
        };
    }

    if widget.downcast_ref::<gtk::Notebook>().is_some() {
        return WidgetInfo::Notebook;
    }

    if widget.downcast_ref::<gtk::Grid>().is_some() {
        return WidgetInfo::Grid;
    }

    if widget.downcast_ref::<gtk::FlowBox>().is_some() {
        return WidgetInfo::FlowBox;
    }

    if widget.downcast_ref::<gtk::Overlay>().is_some() {
        return WidgetInfo::Overlay;
    }

    if widget.downcast_ref::<gtk::Fixed>().is_some() {
        return WidgetInfo::Fixed;
    }

    if let Some(r) = widget.downcast_ref::<gtk::Revealer>() {
        return WidgetInfo::Revealer {
            revealed: r.reveals_child(),
        };
    }

    if let Some(e) = widget.downcast_ref::<gtk::Expander>() {
        return WidgetInfo::Expander {
            expanded: e.is_expanded(),
            label: e.label().map(|s| s.to_string()),
        };
    }

    if let Some(f) = widget.downcast_ref::<gtk::Frame>() {
        return WidgetInfo::Frame {
            label: f.label().map(|s| s.to_string()),
        };
    }

    if widget.downcast_ref::<gtk::AspectFrame>().is_some() {
        return WidgetInfo::AspectFrame;
    }

    if widget.downcast_ref::<gtk::Popover>().is_some() {
        return WidgetInfo::Popover;
    }

    // Input widgets (check before Button since some inherit from it)
    if let Some(e) = widget.downcast_ref::<gtk::SearchEntry>() {
        return WidgetInfo::SearchEntry {
            text: e.text().to_string(),
        };
    }

    if widget.downcast_ref::<gtk::PasswordEntry>().is_some() {
        return WidgetInfo::PasswordEntry;
    }

    if let Some(e) = widget.downcast_ref::<gtk::Entry>() {
        return WidgetInfo::Entry {
            text: e.text().to_string(),
            placeholder: e.placeholder_text().map(|s| s.to_string()),
        };
    }

    if let Some(tv) = widget.downcast_ref::<gtk::TextView>() {
        let buffer = tv.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        return WidgetInfo::TextView { text };
    }

    if let Some(sb) = widget.downcast_ref::<gtk::SpinButton>() {
        return WidgetInfo::SpinButton { value: sb.value() };
    }

    if let Some(s) = widget.downcast_ref::<gtk::Scale>() {
        return WidgetInfo::Scale { value: s.value() };
    }

    if let Some(sw) = widget.downcast_ref::<gtk::Switch>() {
        return WidgetInfo::Switch {
            active: sw.is_active(),
        };
    }

    if let Some(cb) = widget.downcast_ref::<gtk::CheckButton>() {
        return WidgetInfo::CheckButton {
            active: cb.is_active(),
            label: cb.label().map(|s| s.to_string()),
        };
    }

    if let Some(tb) = widget.downcast_ref::<gtk::ToggleButton>() {
        return WidgetInfo::ToggleButton {
            active: tb.is_active(),
            label: tb.label().map(|s| s.to_string()),
        };
    }

    if let Some(mb) = widget.downcast_ref::<gtk::MenuButton>() {
        return WidgetInfo::MenuButton {
            label: mb.label().map(|s| s.to_string()),
        };
    }

    if let Some(lb) = widget.downcast_ref::<gtk::LinkButton>() {
        return WidgetInfo::LinkButton {
            uri: lb.uri().to_string(),
            label: lb.label().map(|s| s.to_string()),
        };
    }

    if widget.downcast_ref::<gtk::ColorButton>().is_some() {
        return WidgetInfo::ColorButton;
    }

    if widget.downcast_ref::<gtk::FontButton>().is_some() {
        return WidgetInfo::FontButton;
    }

    // Regular button (check after specialized buttons)
    if let Some(b) = widget.downcast_ref::<gtk::Button>() {
        return WidgetInfo::Button {
            label: b.label().map(|s| s.to_string()),
        };
    }

    // Display widgets
    if let Some(l) = widget.downcast_ref::<gtk::Label>() {
        return WidgetInfo::Label {
            text: l.text().to_string(),
        };
    }

    if widget.downcast_ref::<gtk::Picture>().is_some() {
        return WidgetInfo::Picture;
    }

    if widget.downcast_ref::<gtk::Image>().is_some() {
        return WidgetInfo::Image;
    }

    if let Some(s) = widget.downcast_ref::<gtk::Spinner>() {
        return WidgetInfo::Spinner {
            spinning: s.is_spinning(),
        };
    }

    if let Some(pb) = widget.downcast_ref::<gtk::ProgressBar>() {
        return WidgetInfo::ProgressBar {
            fraction: pb.fraction(),
        };
    }

    if let Some(lb) = widget.downcast_ref::<gtk::LevelBar>() {
        return WidgetInfo::LevelBar { value: lb.value() };
    }

    if widget.downcast_ref::<gtk::Separator>().is_some() {
        return WidgetInfo::Separator;
    }

    // Selection widgets
    if widget.downcast_ref::<gtk::ComboBoxText>().is_some() {
        return WidgetInfo::ComboBox;
    }

    if widget.downcast_ref::<gtk::DropDown>().is_some() {
        return WidgetInfo::DropDown;
    }

    // Drawing/media
    if widget.downcast_ref::<gtk::DrawingArea>().is_some() {
        return WidgetInfo::DrawingArea;
    }

    if widget.downcast_ref::<gtk::GLArea>().is_some() {
        return WidgetInfo::GLArea;
    }

    if widget.downcast_ref::<gtk::Video>().is_some() {
        return WidgetInfo::Video;
    }

    if widget.downcast_ref::<gtk::MediaControls>().is_some() {
        return WidgetInfo::MediaControls;
    }

    if widget.downcast_ref::<gtk::Calendar>().is_some() {
        return WidgetInfo::Calendar;
    }

    // Unknown widget - get the type name
    WidgetInfo::Unknown {
        type_name: widget.type_().name().to_string(),
    }
}
