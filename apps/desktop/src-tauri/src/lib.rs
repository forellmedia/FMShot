//! Verdrahtung der Desktop-App: Fenster, Tray, Commands.
//! Logik gehört nach `crates/` (siehe CLAUDE.md).

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("FMShot konnte nicht gestartet werden");
}
