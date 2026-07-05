fn main() {
    // declare our commands so the ACL generates allow-$command permissions —
    // the remote panel (vibebuddy.io) may only invoke what its capability grants
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "toggle_panel",
                "quit_app",
                "bubble_menu",
                "get_account",
                "install_agent",
                "connection_report",
            ]),
        ),
    )
    .expect("failed to run tauri-build");
}
