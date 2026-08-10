//! Icon set ported from the design system's `icons.jsx`. Lucide-style, 24x24 viewbox,
//! outlined strokes so a single `color`/`stroke_width` prop can restyle any glyph.

use dioxus::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconName {
    Mic,
    MicOff,
    Video,
    VideoOff,
    Phone,
    PhoneOff,
    Monitor,
    Users,
    MoreHorizontal,
    Search,
    Info,
    Hash,
    Lock,
    LockOpen,
    Paperclip,
    Smile,
    Bold,
    Italic,
    Code,
    Link,
    Send,
    X,
    ArrowLeft,
    Settings,
    ChevronDown,
    ChevronRight,
    Plus,
    Minus,
    Square,
    Check,
    CheckCheck,
    ShieldCheck,
    ShieldAlert,
    Image,
    File,
    Reply,
    MessageSquare,
    Layers,
    Home,
    LogOut,
    Grid,
    Maximize,
}

#[component]
pub fn Icon(
    name: IconName,
    #[props(default = 18)] size: i64,
    #[props(default = None)] color: Option<String>,
    #[props(default = 1.8)] stroke_width: f64,
) -> Element {
    let stroke = color.unwrap_or_else(|| "currentColor".to_string());
    rsx! {
        svg {
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "{stroke}",
            stroke_width: "{stroke_width}",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            style: "flex-shrink:0",
            {icon_glyph(name)}
        }
    }
}

fn icon_glyph(name: IconName) -> Element {
    match name {
        IconName::Mic => rsx! {
            rect { x: "9", y: "2", width: "6", height: "11", rx: "3" }
            path { d: "M5 10a7 7 0 0014 0" }
            line { x1: "12", y1: "17", x2: "12", y2: "21" }
            line { x1: "8", y1: "21", x2: "16", y2: "21" }
        },
        IconName::MicOff => rsx! {
            rect { x: "9", y: "2", width: "6", height: "11", rx: "3" }
            path { d: "M5 10a7 7 0 0014 0" }
            line { x1: "12", y1: "17", x2: "12", y2: "21" }
            line { x1: "8", y1: "21", x2: "16", y2: "21" }
            line { x1: "3", y1: "3", x2: "21", y2: "21" }
        },
        IconName::Video => rsx! {
            rect { x: "2", y: "6", width: "15", height: "12", rx: "2" }
            path { d: "M17 10l5-3v10l-5-3z" }
        },
        IconName::VideoOff => rsx! {
            rect { x: "2", y: "6", width: "15", height: "12", rx: "2" }
            path { d: "M17 10l5-3v10l-5-3z" }
            line { x1: "1", y1: "1", x2: "23", y2: "23" }
        },
        IconName::Phone => rsx! {
            path { d: "M6 3a2 2 0 00-2 2c0 8.837 7.163 16 16 16a2 2 0 002-2v-2.28a1 1 0 00-.76-.97l-3.6-.9a1 1 0 00-1.03.36l-1.2 1.5a13 13 0 01-6.1-6.1l1.5-1.2a1 1 0 00.36-1.03l-.9-3.6A1 1 0 008.28 3H6z" }
        },
        IconName::PhoneOff => rsx! {
            path { opacity: "0.5", d: "M6 3a2 2 0 00-2 2c0 3.5 1 6.8 2.7 9.6" }
            path { d: "M16.5 19.3A16 16 0 0020 17.7v-2.28a1 1 0 00-.76-.97l-3.6-.9a1 1 0 00-1.03.36l-1.2 1.5" }
            line { x1: "3", y1: "3", x2: "21", y2: "21" }
        },
        IconName::Monitor => rsx! {
            rect { x: "2", y: "4", width: "20", height: "13", rx: "2" }
            line { x1: "8", y1: "21", x2: "16", y2: "21" }
            line { x1: "12", y1: "17", x2: "12", y2: "21" }
        },
        IconName::Users => rsx! {
            circle { cx: "9", cy: "8", r: "3.2" }
            path { d: "M2.5 20.5v-.8a5.5 5.5 0 0113 0v.8" }
            path { d: "M16.3 8.5a2.7 2.7 0 110 5.3" }
            path { d: "M17.5 15.2a5 5 0 013.9 4.9v.4" }
        },
        IconName::MoreHorizontal => rsx! {
            circle { cx: "5", cy: "12", r: "1.6", fill: "currentColor", stroke: "none" }
            circle { cx: "12", cy: "12", r: "1.6", fill: "currentColor", stroke: "none" }
            circle { cx: "19", cy: "12", r: "1.6", fill: "currentColor", stroke: "none" }
        },
        IconName::Search => rsx! {
            circle { cx: "11", cy: "11", r: "7" }
            line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
        },
        IconName::Info => rsx! {
            circle { cx: "12", cy: "12", r: "10" }
            line { x1: "12", y1: "16", x2: "12", y2: "11" }
            circle { cx: "12", cy: "7.5", r: "1", fill: "currentColor", stroke: "none" }
        },
        IconName::Hash => rsx! {
            line { x1: "4", y1: "9", x2: "20", y2: "9" }
            line { x1: "4", y1: "15", x2: "20", y2: "15" }
            line { x1: "10", y1: "3", x2: "8", y2: "21" }
            line { x1: "16", y1: "3", x2: "14", y2: "21" }
        },
        IconName::Lock => rsx! {
            rect { x: "3", y: "11", width: "18", height: "10", rx: "2" }
            path { d: "M7 11V7a5 5 0 0110 0v4" }
        },
        IconName::LockOpen => rsx! {
            rect { x: "3", y: "11", width: "18", height: "10", rx: "2" }
            path { d: "M7 11V7a5 5 0 019.9-1.5" }
        },
        IconName::Paperclip => rsx! {
            path { d: "M21 12.5l-8.5 8.5a5 5 0 01-7-7l9-9a3.5 3.5 0 015 5l-8 8a1.5 1.5 0 01-2-2l7-7" }
        },
        IconName::Smile => rsx! {
            circle { cx: "12", cy: "12", r: "10" }
            path { d: "M8 14s1.5 2.2 4 2.2 4-2.2 4-2.2" }
            circle { cx: "9", cy: "9.5", r: "1", fill: "currentColor", stroke: "none" }
            circle { cx: "15", cy: "9.5", r: "1", fill: "currentColor", stroke: "none" }
        },
        IconName::Bold => rsx! {
            text {
                x: "6.5",
                y: "17.5",
                font_size: "15",
                font_weight: "800",
                fill: "currentColor",
                stroke: "none",
                font_family: "var(--font-sans)",
                "B"
            }
        },
        IconName::Italic => rsx! {
            text {
                x: "8",
                y: "17.5",
                font_size: "15",
                font_weight: "700",
                fill: "currentColor",
                stroke: "none",
                font_style: "italic",
                font_family: "var(--font-sans)",
                "I"
            }
        },
        IconName::Code => rsx! {
            text {
                x: "2",
                y: "16",
                font_size: "12",
                font_weight: "700",
                fill: "currentColor",
                stroke: "none",
                font_family: "var(--font-mono)",
                "</>"
            }
        },
        IconName::Link => rsx! {
            path { d: "M9 15l6-6" }
            path { d: "M8.5 8.5l1-1a4 4 0 015.66 5.66l-1 1" }
            path { d: "M15.5 15.5l-1 1a4 4 0 01-5.66-5.66l1-1" }
        },
        IconName::Send => rsx! {
            path { d: "M22 2L11 13M22 2l-7 20-4-9-9-4z" }
        },
        IconName::X => rsx! {
            line { x1: "18", y1: "6", x2: "6", y2: "18" }
            line { x1: "6", y1: "6", x2: "18", y2: "18" }
        },
        IconName::ArrowLeft => rsx! {
            line { x1: "19", y1: "12", x2: "5", y2: "12" }
            path { d: "M12 19l-7-7 7-7" }
        },
        IconName::Settings => rsx! {
            path { d: "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" }
            circle { cx: "12", cy: "12", r: "3" }
        },
        IconName::ChevronDown => rsx! {
            path { d: "M6 9l6 6 6-6" }
        },
        IconName::ChevronRight => rsx! {
            path { d: "M9 6l6 6-6 6" }
        },
        IconName::Plus => rsx! {
            line { x1: "12", y1: "5", x2: "12", y2: "19" }
            line { x1: "5", y1: "12", x2: "19", y2: "12" }
        },
        IconName::Minus => rsx! {
            line { x1: "5", y1: "12", x2: "19", y2: "12" }
        },
        IconName::Square => rsx! {
            rect { x: "6", y: "6", width: "12", height: "12", rx: "1.5" }
        },
        IconName::Check => rsx! {
            path { d: "M20 6L9 17l-5-5" }
        },
        IconName::CheckCheck => rsx! {
            path { d: "M18 6L7 17l-5-5" }
            path { d: "M22 6L11 17" }
        },
        IconName::ShieldCheck => rsx! {
            path { d: "M12 2l8 4v6c0 5-3.5 8-8 10-4.5-2-8-5-8-10V6l8-4z" }
            path { d: "M9 12l2 2 4-4" }
        },
        IconName::ShieldAlert => rsx! {
            path { d: "M12 2l8 4v6c0 5-3.5 8-8 10-4.5-2-8-5-8-10V6l8-4z" }
            line { x1: "12", y1: "8", x2: "12", y2: "13" }
            circle { cx: "12", cy: "16", r: "0.6", fill: "currentColor", stroke: "none" }
        },
        IconName::Image => rsx! {
            rect { x: "3", y: "3", width: "18", height: "18", rx: "2" }
            circle { cx: "8.5", cy: "8.5", r: "1.5" }
            path { d: "M21 15l-5-5-9 9" }
        },
        IconName::File => rsx! {
            path { d: "M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z" }
            path { d: "M14 2v6h6" }
        },
        IconName::Reply => rsx! {
            path { d: "M9 17l-5-5 5-5" }
            path { d: "M4 12h10a6 6 0 016 6v1" }
        },
        IconName::MessageSquare => rsx! {
            path { d: "M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z" }
        },
        IconName::Layers => rsx! {
            path { d: "M12 2l9 5-9 5-9-5 9-5z" }
            path { d: "M3 12l9 5 9-5" }
            path { d: "M3 17l9 5 9-5" }
        },
        IconName::Home => rsx! {
            path { d: "M3 11l9-8 9 8" }
            path { d: "M5 10v10a1 1 0 001 1h4v-6h4v6h4a1 1 0 001-1V10" }
        },
        IconName::LogOut => rsx! {
            path { d: "M9 21H5a2 2 0 01-2-2V5a2 2 0 012-2h4" }
            line { x1: "16", y1: "17", x2: "21", y2: "12" }
            line { x1: "21", y1: "12", x2: "16", y2: "7" }
            line { x1: "21", y1: "12", x2: "9", y2: "12" }
        },
        IconName::Grid => rsx! {
            rect { x: "3", y: "3", width: "7", height: "7", rx: "1" }
            rect { x: "14", y: "3", width: "7", height: "7", rx: "1" }
            rect { x: "14", y: "14", width: "7", height: "7", rx: "1" }
            rect { x: "3", y: "14", width: "7", height: "7", rx: "1" }
        },
        IconName::Maximize => rsx! {
            path { d: "M8 3H5a2 2 0 00-2 2v3" }
            path { d: "M16 3h3a2 2 0 012 2v3" }
            path { d: "M21 16v3a2 2 0 01-2 2h-3" }
            path { d: "M3 16v3a2 2 0 002 2h3" }
        },
    }
}
