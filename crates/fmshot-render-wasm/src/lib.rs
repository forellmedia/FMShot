//! Schnittstelle des Renderers für den Editor (WebAssembly).
//!
//! Der Editor hält ein [`WasmRenderer`], lädt einmal die Bilder (PNG-Bytes)
//! und lässt dann Ausschnitte zeichnen. Dokumente kommen als JSON – dasselbe
//! Format wie in der Bibliothek.

use std::collections::HashMap;

use fmshot_core::{AssetRef, Document, Rect, RgbaImage, TextStyle};
use fmshot_render::{RenderOptions, Renderer};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WasmRenderer {
    renderer: Renderer,
    assets: HashMap<AssetRef, RgbaImage>,
}

/// Gerendertes Bild: RGBA mit gerader Deckkraft, passt direkt in `ImageData`.
#[wasm_bindgen]
pub struct RenderedImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

#[wasm_bindgen]
impl RenderedImage {
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Übergibt die Pixel (ohne Kopie auf Rust-Seite).
    #[wasm_bindgen(js_name = takeData)]
    pub fn take_data(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.data)
    }
}

#[wasm_bindgen]
impl WasmRenderer {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            renderer: Renderer::new(),
            assets: HashMap::new(),
        }
    }

    /// Lädt ein Bild. `hash` leer = Originalaufnahme.
    #[wasm_bindgen(js_name = setAssetPng)]
    pub fn set_asset_png(&mut self, hash: &str, png: &[u8]) -> Result<Vec<u32>, JsError> {
        let image = fmshot_render::decode_png(png)?;
        let size = vec![image.width(), image.height()];
        self.assets.insert(asset_ref(hash), image);
        Ok(size)
    }

    #[wasm_bindgen(js_name = hasAsset)]
    pub fn has_asset(&self, hash: &str) -> bool {
        self.assets.contains_key(&asset_ref(hash))
    }

    #[wasm_bindgen(js_name = clearAssets)]
    pub fn clear_assets(&mut self) {
        self.assets.clear();
    }

    /// Zeichnet einen Ausschnitt.
    ///
    /// - `x, y, w, h`: Ausschnitt in Dokument-Pixeln
    /// - `scale`: Ausgabepixel pro Dokument-Pixel
    /// - `from, to`: Ebenen-Indizes (halboffen); `to` < 0 = bis zum Ende
    /// - `skip`: Ebenen-ID, die ausgelassen wird (0 = keine)
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        doc_json: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        scale: f32,
        from: usize,
        to: i32,
        background: bool,
        skip: u32,
    ) -> Result<RenderedImage, JsError> {
        let doc: Document = serde_json::from_str(doc_json)?;
        let end = if to < 0 {
            doc.layers.len()
        } else {
            to as usize
        };
        let opts = RenderOptions {
            region: Rect::new(x, y, w, h),
            scale,
            layers: from..end,
            background,
            skip: (skip != 0).then_some(fmshot_core::LayerId(skip)),
            interactive: true,
        };
        let img = self.renderer.render_with(&doc, &self.assets, &opts)?;
        Ok(RenderedImage {
            width: img.width(),
            height: img.height(),
            data: img.into_data(),
        })
    }

    /// Misst Text in Dokument-Pixeln: `[breite, höhe, zeilenhöhe]`.
    #[wasm_bindgen(js_name = measureText)]
    pub fn measure_text(&mut self, text: &str, size: f32, bold: bool, max_width: f32) -> Vec<f32> {
        let style = TextStyle {
            size,
            color: fmshot_core::Color::BLACK,
            bold,
            background: None,
            ..Default::default()
        };
        let m = self
            .renderer
            .measure_text(text, &style, (max_width > 0.0).then_some(max_width));
        vec![m.width, m.height, m.line_height]
    }

    /// Liegt über Ebene `index` ein Effekt? Dann muss alles darüber
    /// mitgezeichnet werden.
    #[wasm_bindgen(js_name = hasEffectAbove)]
    pub fn has_effect_above(&self, doc_json: &str, index: usize) -> Result<bool, JsError> {
        let doc: Document = serde_json::from_str(doc_json)?;
        Ok(fmshot_render::has_effect_above(&doc, index))
    }
}

fn asset_ref(hash: &str) -> AssetRef {
    if hash.is_empty() {
        AssetRef::Original
    } else {
        AssetRef::Asset(hash.to_string())
    }
}
