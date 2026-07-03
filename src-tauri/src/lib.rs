// VibeBuddy desktop bubble v0.2.1 — pretty bubble, native context menu,
// browser-based sign-in handoff, draggable clamped panel, magnetic edges.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

const PANEL_W: f64 = 400.0;
const PANEL_H: f64 = 640.0;
const SITE: &str = "https://vibebuddy.io";
const SNAP_PX: i32 = 48;
const MARGIN: i32 = 8;

static FROSTED: AtomicBool = AtomicBool::new(true);

fn open_in_system_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

fn panel_site_url() -> String {
    if FROSTED.load(Ordering::Relaxed) {
        format!("{SITE}/?translucent=1")
    } else {
        format!("{SITE}/")
    }
}

fn position_panel(app: &AppHandle) {
    let (Some(bubble), Some(panel)) = (
        app.get_webview_window("bubble"),
        app.get_webview_window("panel"),
    ) else {
        return;
    };
    let (Ok(bpos), Ok(bsize)) = (bubble.outer_position(), bubble.outer_size()) else {
        return;
    };
    let scale = bubble.scale_factor().unwrap_or(1.0);
    let pw = (PANEL_W * scale) as i32;
    let ph = (PANEL_H * scale) as i32;
    let gap = (10.0 * scale) as i32;

    // monitor bounds (fall back to a large virtual area)
    let (mx, my, mw, mh) = bubble
        .current_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let p = *m.position();
            let s = *m.size();
            (p.x, p.y, s.width as i32, s.height as i32)
        })
        .unwrap_or((0, 0, 1920, 1080));

    let mut x = bpos.x + bsize.width as i32 - pw;
    // prefer above the bubble; if it would clip the top, go below
    let mut y = bpos.y - ph - gap;
    if y < my + MARGIN {
        y = bpos.y + bsize.height as i32 + gap;
    }
    x = x.clamp(mx + MARGIN, (mx + mw - pw - MARGIN).max(mx + MARGIN));
    y = y.clamp(my + MARGIN, (my + mh - ph - MARGIN).max(my + MARGIN));
    let _ = panel.set_position(PhysicalPosition::new(x, y));
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

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

// username only — the bubble draws the account buddy with the same seed as the web
#[tauri::command]
fn get_account() -> serde_json::Value {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    let p = std::path::Path::new(&home).join(".vibebuddy").join("config.json");
    if let Ok(s) = std::fs::read_to_string(p) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            return serde_json::json!({ "username": v.get("username") });
        }
    }
    serde_json::json!({ "username": null })
}

// native right-click menu on the bubble
#[tauri::command]
fn bubble_menu(app: AppHandle) {
    let _ = app.run_on_main_thread({
        let app = app.clone();
        move || {
            let Some(bubble) = app.get_webview_window("bubble") else {
                return;
            };
            let open = MenuItemBuilder::with_id("open", "open / hide panel").build(&app);
            let frosted = CheckMenuItemBuilder::with_id("frosted", "frosted panel")
                .checked(FROSTED.load(Ordering::Relaxed))
                .build(&app);
            let snap_l = MenuItemBuilder::with_id("snap_left", "snap left").build(&app);
            let snap_r = MenuItemBuilder::with_id("snap_right", "snap right").build(&app);
            let quit = MenuItemBuilder::with_id("quit", "quit vibebuddy").build(&app);
            if let (Ok(open), Ok(frosted), Ok(snap_l), Ok(snap_r), Ok(quit)) =
                (open, frosted, snap_l, snap_r, quit)
            {
                if let Ok(menu) = MenuBuilder::new(&app)
                    .item(&open)
                    .item(&frosted)
                    .separator()
                    .item(&snap_l)
                    .item(&snap_r)
                    .separator()
                    .item(&quit)
                    .build()
                {
                    let _ = bubble.popup_menu(&menu);
                }
            }
        }
    });
}

fn snap_bubble(app: &AppHandle, side: &str) {
    let Some(bubble) = app.get_webview_window("bubble") else {
        return;
    };
    let (Ok(pos), Ok(size)) = (bubble.outer_position(), bubble.outer_size()) else {
        return;
    };
    let (mx, _my, mw, _mh) = bubble
        .current_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let p = *m.position();
            let s = *m.size();
            (p.x, p.y, s.width as i32, s.height as i32)
        })
        .unwrap_or((0, 0, 1920, 1080));
    let x = if side == "left" {
        mx + MARGIN
    } else {
        mx + mw - size.width as i32 - MARGIN
    };
    let _ = bubble.set_position(PhysicalPosition::new(x, pos.y));
}

fn handle_menu(app: &AppHandle, id: &str) {
    match id {
        "open" => toggle_panel(app.clone()),
        "frosted" => {
            let now = !FROSTED.load(Ordering::Relaxed);
            FROSTED.store(now, Ordering::Relaxed);
            if let Some(panel) = app.get_webview_window("panel") {
                let _ = panel.eval(&format!("location.replace('{}')", panel_site_url()));
            }
        }
        "snap_left" => snap_bubble(app, "left"),
        "snap_right" => snap_bubble(app, "right"),
        "quit" => app.exit(0),
        _ => {}
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![toggle_panel, quit_app, bubble_menu, get_account])
        .on_menu_event(|app, event| handle_menu(app, event.id().as_ref()))
        .setup(|app| {
            // ---- the bubble ----
            let bubble = WebviewWindowBuilder::new(app, "bubble", WebviewUrl::App("index.html".into()))
                .title("vibebuddy")
                .inner_size(72.0, 72.0)
                .decorations(false)
                .transparent(true)
                .shadow(false) // the white square villain from v0.2.0
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .build()?;
            if let Ok(Some(monitor)) = bubble.primary_monitor() {
                let m = monitor.size();
                let _ = bubble.set_position(PhysicalPosition::new(
                    m.width as i32 - 120,
                    m.height as i32 - 200,
                ));
            }

            // magnetic edges: after a drag settles (~450ms quiet), snap to the near edge
            let last_move: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
            let bubble_for_snap = bubble.clone();
            let last_move_evt = last_move.clone();
            bubble.on_window_event(move |event| {
                if let WindowEvent::Moved(_) = event {
                    let stamp = Instant::now();
                    *last_move_evt.lock().unwrap() = Some(stamp);
                    let bubble = bubble_for_snap.clone();
                    let last_move = last_move_evt.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(450));
                        if *last_move.lock().unwrap() != Some(stamp) {
                            return; // a newer move superseded us
                        }
                        let (Ok(pos), Ok(size)) = (bubble.outer_position(), bubble.outer_size()) else {
                            return;
                        };
                        let Some(monitor) = bubble.current_monitor().ok().flatten() else {
                            return;
                        };
                        let mp = *monitor.position();
                        let ms = *monitor.size();
                        let (mut x, mut y) = (pos.x, pos.y);
                        let w = size.width as i32;
                        let h = size.height as i32;
                        if x - mp.x < SNAP_PX {
                            x = mp.x + MARGIN;
                        } else if (mp.x + ms.width as i32) - (x + w) < SNAP_PX {
                            x = mp.x + ms.width as i32 - w - MARGIN;
                        }
                        y = y.clamp(mp.y + MARGIN, mp.y + ms.height as i32 - h - MARGIN);
                        if x != pos.x || y != pos.y {
                            let _ = bubble.set_position(PhysicalPosition::new(x, y));
                        }
                    });
                }
            });

            // ---- the panel ----
            let panel = WebviewWindowBuilder::new(
                app,
                "panel",
                WebviewUrl::External(panel_site_url().parse().unwrap()),
            )
            .title("VibeBuddy")
            .inner_size(PANEL_W, PANEL_H)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .initialization_script("window.__VB_DESKTOP__ = true;")
            .on_navigation(|url| {
                let s = url.as_str();
                // the sign-in handoff page must open in the user's real browser even though
                // it lives on our own domain — their GitHub session lives out there
                let is_link_handoff = s.contains("/link?code=");
                let ours = !is_link_handoff
                    && (s.starts_with("https://vibebuddy.io")
                        || s.starts_with("https://staging.vibebuddy.io")
                        || s.starts_with("tauri://")
                        || s.starts_with("http://tauri.localhost"));
                if !ours {
                    open_in_system_browser(s);
                }
                ours
            })
            .build()?;
            let panel_for_event = panel.clone();
            panel.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = panel_for_event.hide();
                }
            });

            // ---- tray ----
            let open_item = MenuItemBuilder::with_id("open", "open / hide panel").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "quit vibebuddy").build(app)?;
            let tray_menu = MenuBuilder::new(app).item(&open_item).separator().item(&quit_item).build()?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("vibebuddy — your agent's busy, come hang out")
                .menu(&tray_menu)
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running vibebuddy");
}
