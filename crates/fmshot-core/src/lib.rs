//! Dokumentmodell von FMShot.
//!
//! Ein Dokument ist eine freie Leinwand mit einer Zeichenfläche (Artboard) und
//! einer geordneten Liste von Ebenen. Alles ist eine Ebene – auch der
//! Screenshot selbst (siehe ADR 0004). Dieses Crate macht kein I/O.

mod document;
mod geometry;
mod image;

pub use document::{
    Artboard, AssetRef, Document, Layer, LayerId, LayerKind, SCHEMA_VERSION, Stroke,
};
pub use geometry::{Color, Point, Rect, Size};
pub use image::{ImageError, RgbaImage};
