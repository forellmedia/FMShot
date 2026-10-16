//! Verdrahtung der Desktop-App: Fenster, Tray, Commands.
//! Logik gehört nach `crates/` (siehe CLAUDE.md).

mod commands;
mod updater;

use tauri_specta::{Builder, collect_commands};

/// Alle Commands, die das Frontend aufrufen darf. Die TypeScript-Typen dazu
/// erzeugt `cargo test -p fmshot-desktop` nach `src/lib/bindings.ts`.
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        commands::app_info,
        updater::check_for_update,
        updater::install_update,
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("FMShot konnte nicht gestartet werden");
}

#[cfg(test)]
mod tests {
    use specta_typescript::Typescript;

    /// Schreibt die TypeScript-Bindings. Die CI prüft danach per `git diff`,
    /// ob die eingecheckte Datei aktuell ist.
    #[test]
    fn typescript_bindings_exportieren() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/lib/bindings.ts");
        super::specta_builder()
            .export(
                Typescript::default()
                    .header("// Automatisch erzeugt von tauri-specta – nicht von Hand ändern.\n"),
                path,
            )
            .expect("Bindings konnten nicht geschrieben werden");
    }
}
