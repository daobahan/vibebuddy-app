// VibeBuddy desktop bubble v0.2.1 — pretty bubble, native context menu,
// browser-based sign-in handoff, draggable clamped panel, magnetic edges.
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
static SOLIDITY: AtomicU32 = AtomicU32::new(86); // panel opaqueness %, adjustable from the bubble menu
static CONNECTING: AtomicBool = AtomicBool::new(false); // one agent-install dance at a time

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

    // dock toward the bubble's half of the screen so the pair hugs the near edge
    let bubble_center = bpos.x + bsize.width as i32 / 2;
    let mut x = if bubble_center < mx + mw / 2 {
        bpos.x // left half: panel's left edge lines up with the bubble
    } else {
        bpos.x + bsize.width as i32 - pw // right half: right edges align
    };
    // prefer above the bubble; if it would clip the top, go below
    let mut y = bpos.y - ph - gap;
    if y < my + MARGIN {
        y = bpos.y + bsize.height as i32 + gap;
    }
    x = x.clamp(mx + MARGIN, (mx + mw - pw - MARGIN).max(mx + MARGIN));
    y = y.clamp(my + MARGIN, (my + mh - ph - MARGIN).max(my + MARGIN));
    let _ = panel.set_position(PhysicalPosition::new(x, y));
}

// ---- wiring this machine's agents: connection is the core gameplay, it must not wait for a button ----

fn vb_config_path() -> std::path::PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    std::path::Path::new(&home).join(".vibebuddy").join("config.json")
}

// wired = a config for OUR server with a token in it already lives on this machine
fn machine_wired() -> bool {
    let Ok(s) = std::fs::read_to_string(vb_config_path()) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
        return false;
    };
    v.get("server").and_then(|x| x.as_str()) == Some(SITE)
        && v.get("token").and_then(|x| x.as_str()).is_some_and(|t| t.starts_with("vb_"))
}

fn no_window(cmd: &mut std::process::Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = cmd;
    }
}

// mint an agent token with the panel's own session — curl ships with every OS we target
fn mint_token(sid: &str) -> Result<String, String> {
    let mut c = std::process::Command::new("curl");
    c.args([
        "-s", "-m", "20", "-X", "POST",
        "-H", "Content-Type: application/json",
        "-d", r#"{"agent_kind":"machine"}"#,
    ]);
    c.arg("-H").arg(format!("Cookie: vb_sid={sid}"));
    c.arg(format!("{SITE}/api/tokens"));
    no_window(&mut c);
    let out = c.output().map_err(|e| format!("could not run curl: {e}"))?;
    let body = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(body.trim())
        .map_err(|_| format!("token mint failed: {}", body.chars().take(120).collect::<String>()))?;
    match v.get("token").and_then(|t| t.as_str()) {
        Some(t) => Ok(t.to_string()),
        None => Err(format!(
            "token mint refused: {}",
            v.get("error").and_then(|e| e.as_str()).unwrap_or("unknown error")
        )),
    }
}

fn run_npx_init(token: &str, server: &str) -> Result<String, String> {
    let output = {
        #[cfg(target_os = "windows")]
        {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", "npx", "-y", "vibebuddy@latest", "init", "--token", token, "--server", server]);
            no_window(&mut c);
            c.output()
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::process::Command::new("sh")
                .args(["-lc", &format!("npx -y vibebuddy@latest init --token {token} --server {server}")])
                .output()
        }
    }
    .map_err(|e| format!("could not run npx: {e}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        Ok(text.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" "))
    } else {
        Err(text.chars().take(400).collect())
    }
}

// tell the panel something worth a toast (serde makes the JS string injection-proof)
fn panel_toast(app: &AppHandle, msg: &str) {
    if let Some(panel) = app.get_webview_window("panel") {
        let quoted = serde_json::to_string(msg).unwrap_or_else(|_| "\"\"".into());
        let _ = panel.eval(&format!(
            "window.dispatchEvent(new CustomEvent('vb:toast', {{ detail: {quoted} }}))"
        ));
    }
}

fn config_token_server() -> Option<(String, String)> {
    let s = std::fs::read_to_string(vb_config_path()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    Some((
        v.get("token")?.as_str()?.to_string(),
        v.get("server")?.as_str()?.to_string(),
    ))
}

// codex installed after this machine was wired: its config exists but lacks our MCP bridge
fn codex_unwired() -> bool {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    let toml = std::path::Path::new(&home).join(".codex").join("config.toml");
    match std::fs::read_to_string(toml) {
        Ok(s) => !s.contains("[mcp_servers.vibebuddy]"),
        Err(_) => false, // no codex here — nothing to wire
    }
}

fn home_dir() -> String {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default()
}

// `claude mcp add` fails silently when the claude CLI is not on the GUI PATH —
// write the user-scope registration into ~/.claude.json ourselves (same shape).
fn ensure_claude_mcp() -> bool {
    let home = home_dir();
    let mcp_path = std::path::Path::new(&home).join(".vibebuddy").join("mcp.mjs");
    if !mcp_path.exists() {
        return false;
    }
    let cj = std::path::Path::new(&home).join(".claude.json");
    let Ok(s) = std::fs::read_to_string(&cj) else {
        return false; // no claude on this box — nothing to register
    };
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&s) else {
        return false; // never clobber a file we cannot parse
    };
    let Some(root) = v.as_object_mut() else { return false };
    let servers = root
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let Some(servers) = servers.as_object_mut() else { return false };
    if servers.contains_key("vibebuddy") {
        return true;
    }
    servers.insert(
        "vibebuddy".into(),
        serde_json::json!({
            "type": "stdio",
            "command": "node",
            "args": [mcp_path.to_string_lossy().replace('\\', "/")],
            "env": { "VB_AGENT_KIND": "claude-code" }
        }),
    );
    std::fs::write(&cj, serde_json::to_string_pretty(&v).unwrap_or(s)).is_ok()
}

// the app is the sensor: codex desktop runs as Codex.exe — if it's alive, say so.
// no config handshakes, no restart timing, just an honest process check.
fn codex_running() -> bool {
    let mut c = std::process::Command::new("tasklist");
    c.args(["/FI", "IMAGENAME eq Codex.exe", "/NH"]);
    no_window(&mut c);
    match c.output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_lowercase().contains("codex.exe"),
        Err(_) => false,
    }
}

fn post_agent_event(token: &str, server: &str, body: &str) {
    let mut c = std::process::Command::new("curl");
    c.args(["-s", "-m", "10", "-X", "POST", "-H", "Content-Type: application/json"]);
    c.arg("-H").arg(format!("Authorization: Bearer {token}"));
    c.args(["-d", body]);
    c.arg(format!("{server}/api/agent/event"));
    no_window(&mut c);
    let _ = c.output();
}

// one glance = the whole wiring story (me tab renders this as a checklist)
#[tauri::command]
fn connection_report() -> serde_json::Value {
    let home = home_dir();
    let nest = std::path::Path::new(&home).join(".vibebuddy").join("config.json").exists();
    let hooks = std::fs::read_to_string(std::path::Path::new(&home).join(".claude").join("settings.json"))
        .map(|s| s.matches("hook.mjs").count())
        .unwrap_or(0);
    let claude_mcp = std::fs::read_to_string(std::path::Path::new(&home).join(".claude.json"))
        .map(|s| s.contains("\"vibebuddy\""))
        .unwrap_or(false);
    let codex_cfg = std::fs::read_to_string(std::path::Path::new(&home).join(".codex").join("config.toml"))
        .map(|s| s.contains("[mcp_servers.vibebuddy]"))
        .unwrap_or(false);
    serde_json::json!({
        "nest": nest,
        "claude_hooks": hooks,
        "claude_mcp": claude_mcp,
        "codex_bridge": codex_cfg,
        "codex_running": codex_running(),
    })
}

// the whole dance: panel session cookie -> mint token -> npx init.
// force = explicit user ask: rerun init even when wired (picks up newly installed
// agents like a fresh codex, and refreshes hook templates after CLI updates).
// NEVER call from the main thread — cookies_for_url round-trips through the event loop.
fn ensure_agent_connected(app: &AppHandle, force: bool) -> Result<String, String> {
    if CONNECTING.swap(true, Ordering::SeqCst) {
        return Err("already connecting — give it a few seconds".into());
    }
    let result = (|| {
        if machine_wired() {
            if !force {
                return Ok("this machine is already wired ✓".to_string());
            }
            if let Some((token, server)) = config_token_server() {
                match run_npx_init(&token, &server) {
                    Ok(_) => {
                        ensure_claude_mcp();
                        return Ok("re-wired — hooks & bridges refreshed ✓".into());
                    }
                    Err(e) if e.contains("not accepted") => {} // stale token — fall through and re-mint
                    Err(e) => return Err(e),
                }
            }
        }
        let panel = app.get_webview_window("panel").ok_or("panel not ready yet")?;
        let url: tauri::Url = SITE.parse().map_err(|e| format!("bad url: {e}"))?;
        let cookies = panel
            .cookies_for_url(url)
            .map_err(|e| format!("could not read session: {e}"))?;
        let sid = cookies
            .iter()
            .find(|c| c.name() == "vb_sid")
            .map(|c| c.value().to_string())
            .ok_or("not signed in yet")?;
        let token = mint_token(&sid)?;
        let out = run_npx_init(&token, SITE);
        ensure_claude_mcp(); // the CLI's `claude mcp add` can miss on GUI PATH — belt it
        out
    })();
    CONNECTING.store(false, Ordering::SeqCst);
    result
}

// spawn the CLI with a pre-authorized token — no browser dance, no terminal.
// with no arguments it self-serves: panel session -> token -> npx (async = off the UI thread).
#[tauri::command]
async fn install_agent(
    app: AppHandle,
    token: Option<String>,
    server: Option<String>,
    force: Option<bool>,
) -> Result<String, String> {
    if let (Some(token), Some(server)) = (token, server) {
        if !token.starts_with("vb_") || !token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err("bad token".into());
        }
        let server_ok = server == "https://vibebuddy.io"
            || server == "https://staging.vibebuddy.io"
            || server.starts_with("http://localhost")
            || server.starts_with("http://127.0.0.1");
        if !server_ok {
            return Err("bad server".into());
        }
        return run_npx_init(&token, &server);
    }
    ensure_agent_connected(&app, force.unwrap_or(false))
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
            let connect = MenuItemBuilder::with_id("connect", "connect this machine's agents").build(&app);
            let frosted = CheckMenuItemBuilder::with_id("frosted", "frosted panel")
                .checked(FROSTED.load(Ordering::Relaxed))
                .build(&app);
            let cur = SOLIDITY.load(Ordering::Relaxed);
            let mut ops = Vec::new();
            for v in [100u32, 90, 80, 70] {
                if let Ok(item) = CheckMenuItemBuilder::with_id(format!("solid_{v}"), format!("panel opacity {v}%"))
                    .checked(cur == v || (v == 90 && cur == 86))
                    .build(&app)
                {
                    ops.push(item);
                }
            }
            let snap_l = MenuItemBuilder::with_id("snap_left", "snap left").build(&app);
            let snap_r = MenuItemBuilder::with_id("snap_right", "snap right").build(&app);
            let quit = MenuItemBuilder::with_id("quit", "quit vibebuddy").build(&app);
            if let (Ok(open), Ok(connect), Ok(frosted), Ok(snap_l), Ok(snap_r), Ok(quit)) =
                (open, connect, frosted, snap_l, snap_r, quit)
            {
                let mut b = MenuBuilder::new(&app).item(&open).item(&connect).item(&frosted).separator();
                for item in &ops {
                    b = b.item(item);
                }
                if let Ok(menu) = b
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
        "connect" => {
            let app = app.clone();
            std::thread::spawn(move || {
                panel_toast(&app, "⚡ connecting your coding agents… (~15s)");
                match ensure_agent_connected(&app, true) {
                    Ok(msg) => panel_toast(&app, &format!("✓ {msg}")),
                    Err(e) => panel_toast(&app, &format!("connect failed: {e} — fallback: npx vibebuddy init")),
                }
            });
        }
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
        id if id.starts_with("solid_") => {
            if let Ok(v) = id.trim_start_matches("solid_").parse::<u32>() {
                SOLIDITY.store(v, Ordering::Relaxed);
                if let Some(panel) = app.get_webview_window("panel") {
                    let _ = panel.eval(&format!(
                        "try{{localStorage.setItem('vb:solid','{v}');document.documentElement.style.setProperty('--panel-solid','{v}%');}}catch(e){{}}"
                    ));
                }
            }
        }
        _ => {}
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![toggle_panel, quit_app, bubble_menu, get_account, install_agent, connection_report])
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
                        // the panel is the bubble's shadow — wherever it settles, follow
                        let app = bubble.app_handle();
                        if let Some(panel) = app.get_webview_window("panel") {
                            if panel.is_visible().unwrap_or(false) {
                                position_panel(app);
                            }
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

            // the codex sensor: while codex desktop is alive, its lane stays online —
            // pure process detection, no config handshakes, no restart timing
            std::thread::spawn(move || loop {
                if let Some((token, server)) = config_token_server() {
                    if codex_running() {
                        post_agent_event(
                            &token,
                            &server,
                            r#"{"type":"app_open","agent_kind":"codex","session_id":"codex-app"}"#,
                        );
                    }
                }
                std::thread::sleep(Duration::from_secs(45));
            });

            // startup auto-connect: if this machine isn't wired and the panel already
            // carries a signed-in session, wire it silently — connection IS the gameplay.
            {
                let app = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(6));
                    if machine_wired() {
                        ensure_claude_mcp(); // self-heal the registration every launch
                    }
                    // wired machine, late-arriving codex: one silent re-init writes its MCP bridge
                    if machine_wired() && codex_unwired() {
                        if let Some((token, server)) = config_token_server() {
                            if run_npx_init(&token, &server).is_ok() {
                                panel_toast(&app, "✓ codex wired up — it counts from its next launch");
                            }
                        }
                    }
                    for _ in 0..24 {
                        if machine_wired() {
                            return;
                        }
                        match ensure_agent_connected(&app, false) {
                            Ok(msg) => {
                                panel_toast(&app, &format!("✓ {msg}"));
                                return;
                            }
                            // not signed in yet: keep waiting — the web app re-triggers after sign-in
                            Err(e) if e.contains("not signed in") || e.contains("already connecting") => {}
                            Err(e) => {
                                panel_toast(&app, &format!("agent connect failed: {e} — fallback: npx vibebuddy init"));
                                return;
                            }
                        }
                        std::thread::sleep(Duration::from_secs(10));
                    }
                });
            }

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
