//! Kommandozeile `fmshot` – vor allem für Wayland und eigene Tastenkürzel im
//! Desktop. Reicht den Befehl an die laufende FMShot-App weiter (die App
//! lässt nur eine Instanz zu und übernimmt die Argumente des zweiten Starts).

use std::process::{Command as Process, ExitCode};

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "fmshot", version, about = "FMShot – Screenshots aufnehmen")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Aufnahme auslösen
    Capture {
        #[arg(value_enum)]
        mode: Mode,
    },
    /// Galerie öffnen
    Gallery,
}

#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    Region,
    Screen,
    Window,
    Scroll,
    Text,
}

impl Mode {
    fn arg(self) -> &'static str {
        match self {
            Mode::Region => "region",
            Mode::Screen => "screen",
            Mode::Window => "window",
            Mode::Scroll => "scroll",
            Mode::Text => "text",
        }
    }
}

/// Bundle-ID der App (macOS).
#[cfg(target_os = "macos")]
const BUNDLE_ID: &str = "ch.forellmedia.fmshot";

fn main() -> ExitCode {
    let cli = Cli::parse();
    let args: Vec<&str> = match &cli.command {
        Command::Capture { mode } => vec!["capture", mode.arg()],
        Command::Gallery => vec!["gallery"],
    };
    match launch(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fmshot: {e}");
            ExitCode::from(1)
        }
    }
}

/// Startet die App mit den Argumenten; läuft sie schon, übernimmt die
/// laufende Instanz den Befehl.
fn launch(args: &[&str]) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = Process::new("open")
            .args(["-g", "-b", BUNDLE_ID, "--args"])
            .args(args)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err("FMShot ist nicht installiert (open -b ch.forellmedia.fmshot)".into())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Die App liegt neben dieser Datei oder im Suchpfad.
        let names: &[&str] = if cfg!(windows) {
            &["FMShot.exe", "fmshot-desktop.exe"]
        } else {
            &["fmshot-desktop", "FMShot"]
        };
        let here = std::env::current_exe().ok();
        let dir = here.as_ref().and_then(|p| p.parent());
        let candidates = names
            .iter()
            .filter_map(|n| dir.map(|d| d.join(n)))
            .filter(|p| p.exists())
            .chain(names.iter().map(std::path::PathBuf::from));
        for exe in candidates {
            if Process::new(&exe).args(args).spawn().is_ok() {
                return Ok(());
            }
        }
        Err("FMShot-App nicht gefunden".into())
    }
}
