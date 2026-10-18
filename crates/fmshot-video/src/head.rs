//! Kopfausschnitt der Webcam (ADR 0011, Etappe 3): aus den erkannten
//! Gesichtern (`<id>_faces.json`) einen ruhigen Ausschnitt rechnen.
//!
//! Die Erkennung schwankt von Bild zu Bild um ein paar Pixel. Damit der
//! Ausschnitt nicht zittert, folgt er dem Kopf über eine kritisch gedämpfte
//! Feder und ignoriert kleine Bewegungen (Totzone). Live-Blase und Export
//! rechnen mit derselben Feder.

use fmshot_core::{FaceTrack, Rect, Size};

/// Höhe des Ausschnitts als Vielfaches der Gesichtshöhe (Kopf mit Schultern).
pub const HEAD_ZOOM: f32 = 2.6;
/// Wo die Gesichtsmitte im Ausschnitt liegt (Anteil von oben) – etwas über
/// der Mitte, wie bei einem Porträt.
const FACE_Y: f32 = 0.45;
/// Kleinster Ausschnitt als Anteil der Bildhöhe (sonst wird es zu pixelig).
const MIN_CROP: f32 = 0.3;
/// Federkonstante (rad/s): ~0,8 s bis der Ausschnitt nachgezogen hat.
const OMEGA: f32 = 6.0;
/// Bewegungen unter diesem Anteil der Gesichtsgrösse lösen nichts aus.
const DEAD_ZONE: f32 = 0.12;
/// Schrittweite der Federrechnung in Sekunden.
const STEP: f32 = 1.0 / 60.0;
/// So viele erste Erkennungen bestimmen den festen Ausschnitt (ohne Folgen).
const FIXED_SAMPLES: usize = 20;

/// Lage des Kopfes: Mitte und Höhe des Gesichts als Anteil des Kamerabilds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadPose {
    pub cx: f32,
    pub cy: f32,
    pub size: f32,
}

impl HeadPose {
    pub fn from_face(face: Rect) -> Self {
        Self {
            cx: face.x + face.width / 2.0,
            cy: face.y + face.height / 2.0,
            size: face.height,
        }
    }

    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            cx: a.cx + (b.cx - a.cx) * t,
            cy: a.cy + (b.cy - a.cy) * t,
            size: a.size + (b.size - a.size) * t,
        }
    }
}

/// Ausschnitt des Kamerabilds (0 bis 1) um den Kopf, der ein Ziel mit dem
/// Seitenverhältnis `target_aspect` ohne Verzerren füllt.
pub fn head_crop(pose: HeadPose, camera: Size, target_aspect: f32) -> Rect {
    let camera_aspect = camera.width / camera.height.max(1.0);
    let mut h = (pose.size * HEAD_ZOOM).clamp(MIN_CROP, 1.0);
    let mut w = h * target_aspect / camera_aspect.max(0.01);
    if w > 1.0 {
        h /= w;
        w = 1.0;
    }
    let x = (pose.cx - w / 2.0).clamp(0.0, 1.0 - w);
    let y = (pose.cy - FACE_Y * h).clamp(0.0, 1.0 - h);
    Rect::new(x, y, w, h)
}

/// Kritisch gedämpfte Feder mit Totzone – folgt dem Kopf ohne Zittern.
#[derive(Debug, Clone)]
pub struct HeadFollower {
    pos: HeadPose,
    vel: [f32; 3],
    target: HeadPose,
}

impl HeadFollower {
    pub fn new(start: HeadPose) -> Self {
        Self {
            pos: start,
            vel: [0.0; 3],
            target: start,
        }
    }

    pub fn pose(&self) -> HeadPose {
        self.pos
    }

    /// Neue Erkennung (oder keine) und `dt` Sekunden seit dem letzten Aufruf.
    pub fn update(&mut self, face: Option<HeadPose>, dt: f32) -> HeadPose {
        if let Some(f) = face {
            let moved = (f.cx - self.target.cx)
                .hypot(f.cy - self.target.cy)
                .max((f.size - self.target.size).abs());
            if moved > DEAD_ZONE * self.target.size.max(0.05) {
                self.target = f;
            }
        }
        let mut left = dt.clamp(0.0, 2.0);
        while left > 0.0 {
            let h = left.min(STEP);
            self.step(h);
            left -= h;
        }
        self.pos
    }

    fn step(&mut self, dt: f32) {
        let pos = [self.pos.cx, self.pos.cy, self.pos.size];
        let target = [self.target.cx, self.target.cy, self.target.size];
        let mut next = pos;
        for i in 0..3 {
            // x'' = -2ω x' - ω² (x - Ziel)
            let acc = -2.0 * OMEGA * self.vel[i] - OMEGA * OMEGA * (pos[i] - target[i]);
            self.vel[i] += acc * dt;
            next[i] = pos[i] + self.vel[i] * dt;
        }
        self.pos = HeadPose {
            cx: next[0],
            cy: next[1],
            size: next[2],
        };
    }
}

/// Weg des Kopfausschnitts über das ganze Video (für den Export).
#[derive(Debug, Clone, PartialEq)]
pub struct HeadPath {
    /// (Zeit in ms, Lage), nach Zeit sortiert.
    keys: Vec<(f64, HeadPose)>,
}

impl HeadPath {
    /// `None`, wenn nie ein Gesicht erkannt wurde – dann bleibt es beim
    /// ganzen Kamerabild.
    pub fn new(track: &FaceTrack, follow: bool) -> Option<Self> {
        let mut samples: Vec<_> = track.samples.clone();
        samples.sort_by_key(|s| s.t_ms);
        let first = samples.iter().find_map(|s| s.face)?;
        if !follow {
            let poses: Vec<HeadPose> = samples
                .iter()
                .filter_map(|s| s.face.map(HeadPose::from_face))
                .take(FIXED_SAMPLES)
                .collect();
            let median = |f: fn(&HeadPose) -> f32| {
                let mut v: Vec<f32> = poses.iter().map(f).collect();
                v.sort_by(f32::total_cmp);
                v[v.len() / 2]
            };
            let pose = HeadPose {
                cx: median(|p| p.cx),
                cy: median(|p| p.cy),
                size: median(|p| p.size),
            };
            return Some(Self {
                keys: vec![(0.0, pose)],
            });
        }
        let mut follower = HeadFollower::new(HeadPose::from_face(first));
        let mut keys = Vec::with_capacity(samples.len() + 1);
        let mut last_t = 0.0;
        keys.push((0.0, follower.pose()));
        for s in &samples {
            let t = s.t_ms as f64;
            let pose = follower.update(
                s.face.map(HeadPose::from_face),
                ((t - last_t) / 1000.0) as f32,
            );
            keys.push((t, pose));
            last_t = t;
        }
        Some(Self { keys })
    }

    /// Lage zur Zeit `t_ms` (zwischen zwei Erkennungen weich überblendet).
    pub fn pose_at(&self, t_ms: f64) -> HeadPose {
        let i = self.keys.partition_point(|(t, _)| *t <= t_ms);
        match (i.checked_sub(1).map(|j| self.keys[j]), self.keys.get(i)) {
            (Some((ta, a)), Some(&(tb, b))) if tb > ta => {
                HeadPose::lerp(a, b, ((t_ms - ta) / (tb - ta)) as f32)
            }
            (Some((_, a)), _) => a,
            (None, Some(&(_, b))) => b,
            (None, None) => HeadPose {
                cx: 0.5,
                cy: 0.5,
                size: 1.0,
            },
        }
    }

    pub fn crop(&self, t_ms: f64, camera: Size, target_aspect: f32) -> Rect {
        head_crop(self.pose_at(t_ms), camera, target_aspect)
    }
}

#[cfg(test)]
mod tests {
    use fmshot_core::FaceSample;

    use super::*;

    const CAM: Size = Size::new(1280.0, 720.0);

    fn face(cx: f32, cy: f32, size: f32) -> Option<Rect> {
        let w = size * 9.0 / 16.0;
        Some(Rect::new(cx - w / 2.0, cy - size / 2.0, w, size))
    }

    fn track(points: &[(u32, Option<Rect>)]) -> FaceTrack {
        FaceTrack {
            samples: points
                .iter()
                .map(|&(t_ms, face)| FaceSample { t_ms, face })
                .collect(),
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn ausschnitt_um_den_kopf_fuer_einen_kreis() {
        let pose = HeadPose {
            cx: 0.5,
            cy: 0.4,
            size: 0.2,
        };
        let c = head_crop(pose, CAM, 1.0);
        // Höhe 0.52 der Bildhöhe; quadratisch in Pixeln → Breite 0.52 · 9/16
        assert!(close(c.height, 0.52));
        assert!(close(c.width, 0.52 * 720.0 / 1280.0));
        assert!(close(c.x + c.width / 2.0, 0.5));
        assert!(close(c.y, 0.4 - 0.45 * 0.52));
    }

    #[test]
    fn ausschnitt_bleibt_im_bild() {
        let pose = HeadPose {
            cx: 0.98,
            cy: 0.05,
            size: 0.3,
        };
        let c = head_crop(pose, CAM, 1.0);
        assert!(c.x >= 0.0 && c.right() <= 1.0 + 1e-6);
        assert!(c.y >= 0.0 && c.bottom() <= 1.0 + 1e-6);
    }

    #[test]
    fn zu_breiter_ausschnitt_wird_verkleinert() {
        // Ziel breiter als die Kamera, grosses Gesicht: Breite wäre > 1
        let pose = HeadPose {
            cx: 0.5,
            cy: 0.5,
            size: 0.5,
        };
        let c = head_crop(pose, CAM, 2.4);
        assert!(close(c.width, 1.0));
        assert!(c.height < 1.0);
    }

    #[test]
    fn kleines_zittern_bewegt_nichts() {
        let mut f = HeadFollower::new(HeadPose::from_face(face(0.5, 0.5, 0.2).unwrap()));
        for i in 0..50 {
            let jitter = if i % 2 == 0 { 0.004 } else { -0.004 };
            f.update(face(0.5 + jitter, 0.5, 0.2).map(HeadPose::from_face), 0.1);
        }
        assert!(close(f.pose().cx, 0.5));
    }

    #[test]
    fn feder_folgt_ohne_ueberschwingen() {
        let mut f = HeadFollower::new(HeadPose::from_face(face(0.3, 0.5, 0.2).unwrap()));
        let target = face(0.7, 0.5, 0.2).map(HeadPose::from_face);
        let mut max: f32 = 0.0;
        for _ in 0..30 {
            max = max.max(f.update(target, 0.1).cx);
        }
        assert!(close(f.pose().cx, 0.7));
        assert!(max <= 0.7 + 1e-3, "überschwingt: {max}");
    }

    #[test]
    fn ohne_gesicht_kein_weg() {
        assert_eq!(HeadPath::new(&track(&[(0, None), (100, None)]), true), None);
    }

    #[test]
    fn ohne_folgen_fester_ausschnitt() {
        let path = HeadPath::new(
            &track(&[
                (0, face(0.4, 0.5, 0.2)),
                (100, face(0.42, 0.5, 0.2)),
                (200, face(0.41, 0.5, 0.2)),
                (5000, face(0.9, 0.5, 0.2)),
            ]),
            false,
        )
        .unwrap();
        assert!(close(path.pose_at(0.0).cx, 0.41));
        assert!(close(path.pose_at(6000.0).cx, 0.41));
    }

    #[test]
    fn mit_folgen_wandert_der_ausschnitt() {
        let mut points = vec![(0, face(0.3, 0.5, 0.2))];
        // Gesicht verschwindet kurz – der Ausschnitt bleibt stehen.
        points.push((100, None));
        for i in 2..40 {
            points.push((i * 100, face(0.7, 0.5, 0.2)));
        }
        let path = HeadPath::new(&track(&points), true).unwrap();
        assert!(close(path.pose_at(0.0).cx, 0.3));
        assert!(close(path.pose_at(100.0).cx, 0.3));
        assert!(close(path.pose_at(3900.0).cx, 0.7));
        // dazwischen weich
        let mid = path.pose_at(500.0).cx;
        assert!(mid > 0.32 && mid < 0.7);
    }
}
