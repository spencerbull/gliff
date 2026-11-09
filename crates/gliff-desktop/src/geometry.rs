use eframe::egui::{pos2, vec2, Pos2, Rect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub stream: [u32; 2],
    pub view: [u32; 2],
    pub scale_milli: u32,
}

impl Geometry {
    pub fn validate(self) -> anyhow::Result<Self> {
        anyhow::ensure!(
            self.stream
                .into_iter()
                .all(|v| v > 0 && v <= 4096 && v % 2 == 0)
                && self.view.into_iter().all(|v| v > 0 && v <= 4096)
                && self.view[0] * 2 >= self.stream[0]
                && self.view[1] * 2 >= self.stream[1]
                && (100..=8000).contains(&self.scale_milli),
            "Invalid stream geometry {self:?}"
        );
        Ok(self)
    }
    /// All window coordinates are egui points; DPI cancels in the ratio.
    pub fn fit(self, available: Rect) -> Rect {
        let scale =
            (available.width() / self.view[0] as f32).min(available.height() / self.view[1] as f32);
        Rect::from_center_size(
            available.center(),
            vec2(self.view[0] as f32 * scale, self.view[1] as f32 * scale),
        )
    }
    pub fn remote(self, rect: Rect, point: Pos2) -> Option<[f64; 2]> {
        if !rect.contains(point) || rect.width() <= 0.0 || rect.height() <= 0.0 {
            return None;
        }
        let factor = 1000.0 / self.scale_milli as f64;
        Some([
            ((point.x - rect.min.x) / rect.width()) as f64 * self.stream[0] as f64 * factor,
            ((point.y - rect.min.y) / rect.height()) as f64 * self.stream[1] as f64 * factor,
        ])
    }
    pub fn local(self, rect: Rect, x: f64, y: f64) -> Pos2 {
        pos2(
            rect.min.x
                + (x * self.scale_milli as f64 / 1000.0 / self.stream[0] as f64) as f32
                    * rect.width(),
            rect.min.y
                + (y * self.scale_milli as f64 / 1000.0 / self.stream[1] as f64) as f32
                    * rect.height(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn letterbox_scale_and_dpi_map_to_logical_remote() {
        let g = Geometry {
            stream: [1920, 1080],
            view: [3840, 2160],
            scale_milli: 1500,
        }
        .validate()
        .unwrap();
        for dpi in [1.0, 1.5] {
            let available = Rect::from_min_size(Pos2::ZERO, vec2(1000.0 / dpi, 1000.0 / dpi));
            let rect = g.fit(available);
            assert!(g.remote(rect, pos2(1.0, 1.0)).is_none());
            let point = g.remote(rect, rect.center()).unwrap();
            assert!((point[0] - 640.0).abs() < 0.001 && (point[1] - 360.0).abs() < 0.001);
            assert!(g.local(rect, 640.0, 360.0).distance(rect.center()) < 0.001);
        }
    }
    #[test]
    fn refuses_hostile_dimensions() {
        for stream in [[0, 2], [4098, 2], [1, 2], [u32::MAX, u32::MAX]] {
            assert!(Geometry {
                stream,
                view: [2, 2],
                scale_milli: 1000
            }
            .validate()
            .is_err());
        }
    }
}
