// VibeBuddy desktop bubble — a tiny always-on-top pet that toggles a floating panel.
use tauri::{
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

const PANEL_W: f64 = 400.0;
const PANEL_H: f64 = 640.0;
const SITE: &str = "https://vibebuddy.io";

fn panel_url(frosted: bool) -> WebviewUrl {
    let url = if frosted {
        format!("{SITE}/?translucent=1")
    } else {
        format!("{SITE}/")
    };
    WebviewUrl::External(url.parse().unwrap())
}

fn position_panel(app: &AppHandle) {
    let (Some(bubble), Some(panel)) = (
        app.get_webview_window("bubble"),
        app.get_webview_window("panel"),
    ) else {
        return;
    };
    if let (Ok(bpos), Ok(bsize)) = (bubble.outer_position(), bubble.outer_size()) {
        let scale = bubble.scale_factor().unwrap_or(1.0);
        let pw = (PANEL_W * scale) as i32;
        let ph = (PANEL_H * scale) as i32;
        let x = bpos.x + bsize.width as i32 - pw;
        let y = bpos.y - ph - (10.0 * scale) as i32;
        let _ = panel.set_position(PhysicalPosition::new(x.max(0), y.max(0)));
    }
}

#[tauri::command]
fn toggle_panel(app: AppHandle) {
    if let Some(panel) = app.get_webview_window("panel") {
        if panel.is_visible().unwrap_or(false) {
            let _ = panel.hide();
        } else {
            position_panel(&app);
            let _ = panel.show();
            let _ = panel.set_focus();
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![toggle_panel])
        .setup(|app| {
            // the bubble: tiny, frameless, transparent, always on top
            let bubble = WebviewWindowBuilder::new(app, "bubble", WebviewUrl::App("index.html".into()))
                .title("vibebuddy")
                .inner_size(72.0, 72.0)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .build()?;
            if let Ok(Some(monitor)) = bubble.primary_monitor() {
                let m = monitor.size();
                let _ = bubble.set_position(PhysicalPosition::new(
                    m.width as i32 - 130,
                    m.height as i32 - 190,
                ));
            }

            // the panel: our 380px sidecar UI, frosted by default, hidden until asked
            let panel = WebviewWindowBuilder::new(app, "panel", panel_url(true))
                .title("VibeBuddy")
                .inner_size(PANEL_W, PANEL_H)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .visible(false)
                .build()?;
            // closing the panel hides it — the bubble is the app's life
            let panel_for_event = panel.clone();
            panel.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = panel_for_event.hide();
                }
            });

            // tray: open, frosted toggle, quit
            let open_item = MenuItemBuilder::with_id("open", "open panel").build(app)?;
            let frosted_item = CheckMenuItemBuilder::with_id("frosted", "frosted panel")
                .checked(true)
                .build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&open_item)
                .item(&frosted_item)
                .separator()
                .item(&quit_item)
                .build()?;
            let frosted_for_menu = frosted_item.clone();
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("vibebuddy — your agent's busy, come hang out")
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "open" => toggle_panel(app.clone()),
                    "frosted" => {
                        let frosted = frosted_for_menu.is_checked().unwrap_or(true);
                        if let Some(panel) = app.get_webview_window("panel") {
                            let url = if frosted {
                                format!("{SITE}/?translucent=1")
                            } else {
                                format!("{SITE}/")
                            };
                            let _ = panel.navigate(url.parse().unwrap());
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running vibebuddy");
}
