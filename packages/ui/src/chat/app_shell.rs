use dioxus::prelude::*;

#[component]
pub fn AppShell(children: Element) -> Element {
    rsx! {
        div { style: "width:100%;height:100%;display:flex;background:var(--bg-canvas);font-family:var(--font-sans);color:var(--text-primary);",
            {children}
        }
    }
}
