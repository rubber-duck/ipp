#[derive(Clone, Copy, Debug)]
pub(super) struct Constraints {
    pub min_w: f32,
    pub min_h: f32,
    pub max_w: f32,
    pub max_h: f32,
}

impl Constraints {
    pub fn loose(max_w: f32, max_h: f32) -> Self {
        Self {
            min_w: 0.0,
            min_h: 0.0,
            max_w,
            max_h,
        }
    }

    pub fn clamp_width(&self, value: f32) -> f32 {
        value.clamp(self.min_w, self.max_w)
    }

    pub fn clamp_height(&self, value: f32) -> f32 {
        value.clamp(self.min_h, self.max_h)
    }
}

pub(super) fn fill_or_fit(content: f32, max: f32) -> f32 {
    if max.is_finite() {
        max.max(0.0)
    } else {
        content.max(0.0)
    }
}

pub(super) fn align_factor(value: Option<f32>, default: f32) -> f32 {
    match value {
        Some(value) if value.is_finite() => ((value + 1.0) / 2.0).clamp(0.0, 1.0),
        _ => ((default + 1.0) / 2.0).clamp(0.0, 1.0),
    }
}
