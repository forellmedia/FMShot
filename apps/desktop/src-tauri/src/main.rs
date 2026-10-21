// Verhindert unter Windows im Release ein zusätzliches Konsolenfenster – NICHT ENTFERNEN.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    grabmark_desktop_lib::run()
}
