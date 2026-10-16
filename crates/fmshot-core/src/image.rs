/// Rohes Bild im Speicher: RGBA8, gerade Deckkraft, Zeilen ohne Auffüllung.
///
/// Gemeinsames Austauschformat zwischen Aufnahme, Zusammensetzen, OCR und
/// Renderer.
#[derive(Clone, PartialEq, Eq)]
pub struct RgbaImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ImageError {
    #[error("Bilddaten haben {actual} Bytes, erwartet waren {expected} ({width}×{height}×4)")]
    WrongLength {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },
}

impl RgbaImage {
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Result<Self, ImageError> {
        let expected = width as usize * height as usize * 4;
        if data.len() != expected {
            return Err(ImageError::WrongLength {
                width,
                height,
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// Bild in einer einzigen Farbe – vor allem für Tests.
    pub fn filled(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let data = rgba
            .iter()
            .copied()
            .cycle()
            .take(width as usize * height as usize * 4)
            .collect();
        Self {
            width,
            height,
            data,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn into_data(self) -> Vec<u8> {
        self.data
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        Some([
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ])
    }
}

impl std::fmt::Debug for RgbaImage {
    // Die Pixel nicht ausgeben – bei 4K wären das Millionen Zahlen.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RgbaImage({}×{})", self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falsche_laenge_wird_abgelehnt() {
        let err = RgbaImage::new(2, 2, vec![0; 15]).unwrap_err();
        assert!(matches!(err, ImageError::WrongLength { expected: 16, .. }));
    }

    #[test]
    fn pixel_liest_richtige_stelle() {
        let mut data = vec![0; 2 * 2 * 4];
        data[12..16].copy_from_slice(&[1, 2, 3, 4]);
        let img = RgbaImage::new(2, 2, data).unwrap();
        assert_eq!(img.pixel(1, 1), Some([1, 2, 3, 4]));
        assert_eq!(img.pixel(2, 0), None);
    }
}
