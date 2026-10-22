//! Dokumentmodell von Nabshot.
//!
//! Ein Dokument ist eine freie Leinwand mit einer Zeichenfläche (Artboard) und
//! einer geordneten Liste von Ebenen. Alles ist eine Ebene – auch der
//! Screenshot selbst (siehe ADR 0004). Dieses Crate macht kein I/O.

mod document;
mod geometry;
mod image;
mod video;

pub use document::{
    ArrowHead, Artboard, AssetRef, Corner, Document, Fill, Layer, LayerId, LayerKind, LineStyle,
    SCHEMA_VERSION, Shadow, Stroke, TextAlign, TextStyle, VerticalAlign,
};
pub use geometry::{Color, Point, Rect, Size};
pub use image::{ImageError, RgbaImage};
pub use video::{
    AudioMix, CameraFormat, CheckpointDrawing, CursorStyle, FaceSample, FaceTrack, Fade,
    InputSample, InputTrack, LayerTime, RecordingCheckpoint, TimeRange, VideoCodec, VideoDoc,
    VideoEncoding, VideoQuality, VideoResolution, VolumeKey, WebcamBackground, WebcamBorder,
    WebcamFraming, WebcamLayout, WebcamMode, WebcamPosition, WebcamSection, WebcamShape,
    WebcamTrack, ZoomSection,
};
