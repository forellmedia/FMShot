//! Kommandozeile `fmshot` – vor allem für Wayland, wo eigene Tastenkürzel im
//! Desktop auf diese Befehle zeigen. Leitet die Befehle später an die laufende
//! App weiter.

use std::process::ExitCode;

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

fn main() -> ExitCode {
    let cli = Cli::parse();
    let what = match cli.command {
        Command::Capture { mode } => match mode {
            Mode::Region => "Bereich aufnehmen",
            Mode::Screen => "Vollbild aufnehmen",
            Mode::Window => "Fenster aufnehmen",
            Mode::Scroll => "Scroll-Aufnahme",
            Mode::Text => "Text erfassen",
        },
        Command::Gallery => "Galerie öffnen",
    };
    eprintln!("fmshot: „{what}“ ist noch nicht umgesetzt.");
    ExitCode::from(2)
}
