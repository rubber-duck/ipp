use crate::ErrorReason;
use crate::components::schema::ComponentLifecycle;
use ipp_schema_derive::SchemaComponent;

/// Constraint layout on one ordinary entity; structure is exclusively core ordered links.
/// Padding reduces measurement constraints and offsets content without adding to
/// intrinsic or explicit outer size, following the existing GUI box rules.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct GuiLayout {
    /// Layout operation: leaf 0, row 1, column 2, stack 3, padding 4, align 5, sized box 6.
    pub kind: u32,
    /// Explicit logical width, or -1 for intrinsic/container sizing.
    pub width: f32,
    /// Explicit logical height, or -1 for intrinsic/container sizing.
    pub height: f32,
    /// Minimum logical width.
    pub min_width: f32,
    /// Minimum logical height.
    pub min_height: f32,
    /// Maximum logical width, or -1 for unbounded.
    pub max_width: f32,
    /// Maximum logical height, or -1 for unbounded.
    pub max_height: f32,
    /// Share of remaining row/column space; zero selects intrinsic measurement.
    pub flex: f32,
    /// Horizontal alignment in -1..=1; 2 selects the container's default.
    pub align_x: f32,
    /// Vertical alignment in -1..=1; 2 selects the container's default.
    pub align_y: f32,
    /// Top inner logical inset.
    pub padding_top: f32,
    /// Right inner logical inset.
    pub padding_right: f32,
    /// Bottom inner logical inset.
    pub padding_bottom: f32,
    /// Left inner logical inset.
    pub padding_left: f32,
    /// Top outer logical margin; negative margins are permitted.
    pub margin_top: f32,
    /// Right outer logical margin; negative margins are permitted.
    pub margin_right: f32,
    /// Bottom outer logical margin; negative margins are permitted.
    pub margin_bottom: f32,
    /// Left outer logical margin; negative margins are permitted.
    pub margin_left: f32,
    /// Clip this entity and its descendants to the settled logical box.
    pub clip: bool,
}

impl Default for GuiLayout {
    fn default() -> Self {
        Self {
            kind: 0,
            width: -1.0,
            height: -1.0,
            min_width: 0.0,
            min_height: 0.0,
            max_width: -1.0,
            max_height: -1.0,
            flex: 0.0,
            align_x: 2.0,
            align_y: 2.0,
            padding_top: 0.0,
            padding_right: 0.0,
            padding_bottom: 0.0,
            padding_left: 0.0,
            margin_top: 0.0,
            margin_right: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            clip: false,
        }
    }
}

impl GuiLayout {
    pub(super) fn padding(self) -> [f32; 4] {
        [
            self.padding_top,
            self.padding_right,
            self.padding_bottom,
            self.padding_left,
        ]
    }

    pub(super) fn margin(self) -> [f32; 4] {
        [
            self.margin_top,
            self.margin_right,
            self.margin_bottom,
            self.margin_left,
        ]
    }
}

impl ComponentLifecycle for GuiLayout {
    fn validate(&self) -> Result<(), ErrorReason> {
        let optional = [self.width, self.height, self.max_width, self.max_height];
        let lengths = [self.min_width, self.min_height, self.flex];
        let aligns = [self.align_x, self.align_y];
        if self.kind > 6
            || optional
                .iter()
                .any(|value| !value.is_finite() || (*value < 0.0 && *value != -1.0))
            || lengths
                .iter()
                .chain(self.padding().iter())
                .any(|value| !value.is_finite() || *value < 0.0)
            || self.margin().iter().any(|value| !value.is_finite())
            || aligns
                .iter()
                .any(|value| !value.is_finite() || (!(-1.0..=1.0).contains(value) && *value != 2.0))
            || (self.max_width >= 0.0 && self.min_width > self.max_width)
            || (self.max_height >= 0.0 && self.min_height > self.max_height)
        {
            Err(ErrorReason::InvalidValue)
        } else {
            Ok(())
        }
    }
}
