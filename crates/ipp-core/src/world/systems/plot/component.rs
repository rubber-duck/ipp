//! Authored Plot values. Column names select this entity's completed binding outputs.

use crate::ErrorReason;
use crate::components::rows::{Rows, SchemaRow};
use crate::components::schema::{ComponentAssetReference, ComponentLifecycle};
use crate::services::asset_management::AssetDemandSelection;
use crate::services::asset_management::formats::font::FONT_TYPE;
use crate::services::data::DataRowId;
use ipp_schema_derive::SchemaComponent;
use std::collections::BTreeSet;
use std::sync::Arc;

/// One series. Its stable row slot is its picking and label identity.
/// Empty optional selectors use the chart's documented constant defaults.
#[derive(Clone, Debug, PartialEq, SchemaRow)]
pub struct PlotSeriesRow {
    /// Client-authored legend text.
    #[schema(text = 256)]
    pub name: Arc<str>,
    /// Horizontal coordinate output.
    #[schema(text = 256)]
    pub x: Arc<str>,
    /// Vertical coordinate output (height in 3D).
    #[schema(text = 256)]
    pub y: Arc<str>,
    /// Depth coordinate output.
    #[schema(text = 256)]
    pub z: Arc<str>,
    /// Bar value or pie share output.
    #[schema(text = 256)]
    pub value: Arc<str>,
    /// Optional variable pie radius output.
    #[schema(text = 256)]
    pub radius: Arc<str>,
    /// Optional variable pie height output.
    #[schema(text = 256)]
    pub height: Arc<str>,
    /// Optional Vec3/Vec4 per-row linear colour output.
    #[schema(text = 256)]
    pub color_column: Arc<str>,
    /// Constant straight linear RGBA when no colour output is selected.
    pub color: [f32; 4],
    /// Whether this series participates in bounds and presentation.
    pub visible: bool,
}

impl Default for PlotSeriesRow {
    fn default() -> Self {
        Self {
            name: Arc::default(),
            x: "x".into(),
            y: "y".into(),
            z: "z".into(),
            value: "value".into(),
            radius: Arc::default(),
            height: Arc::default(),
            color_column: Arc::default(),
            color: [0.0, 0.8, 1.0, 1.0],
            visible: true,
        }
    }
}

/// An explicitly authored label or highlight, qualified by its series slot.
/// Rows have no u64 scalar type: `row_id` is the canonical decimal representation
/// of the full source-row u64, never a floating point number. Typed clients accept
/// bigint and stringify it. Picking still carries a native u64.
#[derive(Clone, Debug, PartialEq, SchemaRow)]
pub struct PlotLabelRow {
    /// Stable slot of the selected PlotSeriesRow.
    pub series: u32,
    /// Exact source-row identity, decimal digits without leading zeroes.
    #[schema(text = 20)]
    pub row_id: Arc<str>,
    /// Preserved client-authored text; empty text permits highlight-only rows.
    #[schema(text = 4096)]
    pub text: Arc<str>,
    /// Whether the mark is highlighted.
    pub highlighted: bool,
    /// Label displacement, +Y down; 3D pies use its magnitude as radial clearance.
    pub offset: [f32; 2],
    /// Whether to connect the label to the mark.
    pub connector: bool,
}

impl Default for PlotLabelRow {
    fn default() -> Self {
        Self {
            series: 0,
            row_id: "0".into(),
            text: Arc::default(),
            highlighted: false,
            offset: [0.0; 2],
            connector: true,
        }
    }
}

impl PlotLabelRow {
    /// Parse the exact identity; malformed and noncanonical values fail authoring.
    pub fn source_row(&self) -> Result<DataRowId, ErrorReason> {
        let id = self
            .row_id
            .parse::<u64>()
            .map_err(|_| ErrorReason::InvalidValue)?;
        if self.row_id.as_ref() != id.to_string() {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(DataRowId(id))
    }
}

/// One linear Cartesian frame shared by all series on this chart entity.
/// Pies use its extent/font/style while retaining independent radial layout.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct PlotFrame2d {
    /// Local logical extent, before CanvasStyle placement.
    pub width: f32,
    /// Local logical height.
    pub height: f32,
    /// Left, top, right and bottom margins in logical units.
    pub padding_left: f32,
    /// Top margin.
    pub padding_top: f32,
    /// Right margin.
    pub padding_right: f32,
    /// Bottom margin.
    pub padding_bottom: f32,
    /// Fixed lower bounds when the corresponding automatic flag is false.
    pub min_x: f32,
    /// Fixed vertical lower bound.
    pub min_y: f32,
    /// Fixed upper bounds.
    pub max_x: f32,
    /// Fixed vertical upper bound.
    pub max_y: f32,
    /// Derive horizontal bounds from every visible series.
    pub automatic_x: bool,
    /// Derive vertical bounds from every visible series.
    pub automatic_y: bool,
    /// Number of evenly spaced axis intervals.
    pub ticks: u32,
    /// Horizontal axis title.
    pub x_title: Arc<str>,
    /// Vertical axis title.
    pub y_title: Arc<str>,
    /// Immutable shared font source for frame and chart labels.
    pub source: Arc<str>,
    /// Font variant.
    pub variant: u32,
    /// Logical units per em.
    pub font_size: f32,
    /// Straight linear RGBA for axes and label text.
    pub red: f32,
    /// Linear green.
    pub green: f32,
    /// Linear blue.
    pub blue: f32,
    /// Linear coverage alpha.
    pub alpha: f32,
    /// Straight linear RGBA for grid lines.
    pub grid_red: f32,
    /// Grid linear green.
    pub grid_green: f32,
    /// Grid linear blue.
    pub grid_blue: f32,
    /// Grid linear coverage alpha.
    pub grid_alpha: f32,
    /// Grid/axis line thickness in local logical units.
    pub line_width: f32,
}

impl Default for PlotFrame2d {
    fn default() -> Self {
        Self {
            width: 600.0,
            height: 320.0,
            padding_left: 56.0,
            padding_top: 24.0,
            padding_right: 20.0,
            padding_bottom: 48.0,
            min_x: 0.0,
            min_y: 0.0,
            max_x: 10.0,
            max_y: 100.0,
            automatic_x: true,
            automatic_y: true,
            ticks: 5,
            x_title: Arc::default(),
            y_title: Arc::default(),
            source: Arc::default(),
            variant: 0,
            font_size: 14.0,
            red: 0.45,
            green: 0.8,
            blue: 1.0,
            alpha: 1.0,
            grid_red: 0.04,
            grid_green: 0.18,
            grid_blue: 0.24,
            grid_alpha: 0.7,
            line_width: 1.0,
        }
    }
}

/// Entity-local 3D linear frame; +X right, +Y height and +Z depth.
/// Text is prepared on ordinary scene-depth planes by Plot, without child Worlds.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct PlotFrame3d {
    /// Local physical axis extents in metres.
    pub width: f32,
    /// Local physical height.
    pub height: f32,
    /// Local physical depth.
    pub depth: f32,
    /// Fixed lower bounds.
    pub min_x: f32,
    /// Fixed height lower bound.
    pub min_y: f32,
    /// Fixed depth lower bound.
    pub min_z: f32,
    /// Fixed upper bounds.
    pub max_x: f32,
    /// Fixed height upper bound.
    pub max_y: f32,
    /// Fixed depth upper bound.
    pub max_z: f32,
    /// Derive X bounds from visible series.
    pub automatic_x: bool,
    /// Derive height bounds from visible series.
    pub automatic_y: bool,
    /// Derive depth bounds from visible series.
    pub automatic_z: bool,
    /// Allow selected-view axes to adapt while the client enables chart interaction.
    /// False preserves the last displayed perimeter station, initially the standard edges.
    pub adaptive_axes: bool,
    /// Number of evenly spaced axis intervals.
    pub ticks: u32,
    /// X axis title.
    pub x_title: Arc<str>,
    /// Height axis title.
    pub y_title: Arc<str>,
    /// Depth axis title.
    pub z_title: Arc<str>,
    /// Immutable shared font source.
    pub source: Arc<str>,
    /// Font variant.
    pub variant: u32,
    /// Metres per em on text planes.
    pub font_size: f32,
    /// Straight linear RGBA for axes and text.
    pub red: f32,
    /// Linear green.
    pub green: f32,
    /// Linear blue.
    pub blue: f32,
    /// Linear coverage alpha.
    pub alpha: f32,
    /// Straight linear RGBA for grid lines.
    pub grid_red: f32,
    /// Grid linear green.
    pub grid_green: f32,
    /// Grid linear blue.
    pub grid_blue: f32,
    /// Grid linear coverage alpha.
    pub grid_alpha: f32,
    /// Local physical line width.
    pub line_width: f32,
}

impl Default for PlotFrame3d {
    fn default() -> Self {
        Self {
            width: 10.0,
            height: 5.0,
            depth: 10.0,
            min_x: 0.0,
            min_y: 0.0,
            min_z: 0.0,
            max_x: 10.0,
            max_y: 100.0,
            max_z: 10.0,
            automatic_x: true,
            automatic_y: true,
            automatic_z: true,
            adaptive_axes: false,
            ticks: 5,
            x_title: Arc::default(),
            y_title: Arc::default(),
            z_title: Arc::default(),
            source: Arc::default(),
            variant: 0,
            font_size: 0.25,
            red: 0.45,
            green: 0.8,
            blue: 1.0,
            alpha: 1.0,
            grid_red: 0.04,
            grid_green: 0.18,
            grid_blue: 0.24,
            grid_alpha: 0.7,
            line_width: 0.015,
        }
    }
}

macro_rules! frame_lifecycle {
    ($name:ty) => {
        impl ComponentLifecycle for $name {
            fn asset_references() -> &'static [ComponentAssetReference] {
                &[ComponentAssetReference {
                    kind: FONT_TYPE.0,
                    source_offset: std::mem::offset_of!(Self, source) as u32,
                    variant_offset: std::mem::offset_of!(Self, variant) as u32,
                }]
            }

            fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
                if !self.source.is_empty() {
                    AssetDemandSelection::insert_into(
                        demand,
                        FONT_TYPE,
                        &self.source,
                        self.variant,
                    );
                }
            }

            fn validate_field(&self, _offset: u32) -> Result<(), ErrorReason> {
                self.validate()
            }

            fn validate(&self) -> Result<(), ErrorReason> {
                positive(&self.size())?;
                positive(&[self.font_size, self.line_width])?;
                color(self.color())?;
                color(self.grid_color())?;
                if self.ticks == 0
                    || self.ticks > 100
                    || self
                        .min()
                        .iter()
                        .zip(self.max())
                        .any(|(min, max)| !min.is_finite() || !max.is_finite() || *min >= max)
                {
                    return Err(ErrorReason::InvalidValue);
                }
                Ok(())
            }
        }
    };
}

frame_lifecycle!(PlotFrame2d);
frame_lifecycle!(PlotFrame3d);

macro_rules! chart_component {
    ($name:ident, $frame:ident, $required:ident, { $( $(#[$doc:meta])* $field:ident : $ty:ty = $default:expr ),* $(,)? }) => {
        /// Data-bound chart presentation. Series and labels are compact rows, not entities.
        #[repr(C)]
        #[derive(Clone, Debug, PartialEq, SchemaComponent)]
        pub struct $name {
            /// Named prepared outputs from the binding on this entity.
            #[schema(rows)]
            pub series: Rows<PlotSeriesRow>,
            /// Explicit source-row labels and highlights.
            #[schema(rows)]
            pub labels: Rows<PlotLabelRow>,
            $( $(#[$doc])* pub $field: $ty, )*
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    series: Rows::default(),
                    labels: Rows::default(),
                    $( $field: $default, )*
                }
            }
        }

        impl ComponentLifecycle for $name {
            fn required_components() -> &'static [u16] {
                &[crate::ComponentValue::$frame, crate::ComponentValue::$required]
            }

            fn validate_field(&self, _offset: u32) -> Result<(), ErrorReason> {
                self.validate()
            }

            fn validate(&self) -> Result<(), ErrorReason> {
                validate_rows(&self.series, &self.labels)?;
                self.validate_chart()
            }
        }
    };
}

chart_component!(PlotLine2d, PLOT_FRAME2D, CANVAS_BOUNDS, {
    /// 0 straight, 1 smooth; controls are derived in Rust from the same samples.
    interpolation: u32 = 0,
    /// Stroke width in local logical units.
    line_width: f32 = 2.0,
    /// Sample marker extent in local logical units; zero omits markers.
    marker_size: f32 = 6.0,
});

chart_component!(PlotBars2d, PLOT_FRAME2D, CANVAS_BOUNDS, {
    /// Fraction of each category cell left empty, in [0,1).
    gap: f32 = 0.2,
});

chart_component!(PlotPie2d, PLOT_FRAME2D, CANVAS_BOUNDS, {
    /// Clockwise radians from twelve o'clock.
    start_angle: f32 = 0.0,
    /// Fraction of outer radius used by the hole, in [0,1).
    inner_radius: f32 = 0.0,
});

chart_component!(PlotGridBars3d, PLOT_FRAME3D, BOUNDING_GEOMETRY, {
    /// Bar width along X in physical units.
    bar_width: f32 = 0.7,
    /// Bar depth along Z in physical units.
    bar_depth: f32 = 0.7,
});

chart_component!(PlotHeightSurface3d, PLOT_FRAME3D, BOUNDING_GEOMETRY, {
    /// Whether to show derived triangle edges.
    wireframe: bool = true,
    /// Local physical triangle-edge thickness.
    line_width: f32 = 0.015,
});

chart_component!(PlotPoints3d, PLOT_FRAME3D, BOUNDING_GEOMETRY, {
    /// Marker extent in local physical units.
    marker_size: f32 = 0.15,
    /// 0 cubes, 1 octahedra.
    marker_shape: u32 = 0,
});

chart_component!(PlotPie3d, PLOT_FRAME3D, BOUNDING_GEOMETRY, {
    /// Clockwise radians around +Y from +Z.
    start_angle: f32 = 0.0,
    /// Constant physical radius when series.radius is empty.
    radius: f32 = 3.0,
    /// Constant physical height when series.height is empty.
    height: f32 = 0.75,
});

impl PlotLine2d {
    fn validate_chart(&self) -> Result<(), ErrorReason> {
        positive(&[self.line_width])?;
        if self.interpolation > 1 || !self.marker_size.is_finite() || self.marker_size < 0.0 {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }
}

impl PlotBars2d {
    fn validate_chart(&self) -> Result<(), ErrorReason> {
        fraction(self.gap)
    }
}

impl PlotPie2d {
    fn validate_chart(&self) -> Result<(), ErrorReason> {
        fraction(self.inner_radius)?;
        finite(self.start_angle)
    }
}

impl PlotGridBars3d {
    fn validate_chart(&self) -> Result<(), ErrorReason> {
        positive(&[self.bar_width, self.bar_depth])
    }
}

impl PlotHeightSurface3d {
    fn validate_chart(&self) -> Result<(), ErrorReason> {
        positive(&[self.line_width])
    }
}

impl PlotPoints3d {
    fn validate_chart(&self) -> Result<(), ErrorReason> {
        positive(&[self.marker_size])?;
        if self.marker_shape > 1 {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }
}

impl PlotPie3d {
    fn validate_chart(&self) -> Result<(), ErrorReason> {
        positive(&[self.radius, self.height])?;
        finite(self.start_angle)
    }
}

fn validate_rows(
    series: &Rows<PlotSeriesRow>,
    labels: &Rows<PlotLabelRow>,
) -> Result<(), ErrorReason> {
    for (_, row) in series.iter() {
        color(row.color)?;
    }
    for (_, row) in labels.iter() {
        row.source_row()?;
        if row.offset.iter().any(|value| !value.is_finite()) {
            return Err(ErrorReason::InvalidValue);
        }
    }
    Ok(())
}

fn color(value: [f32; 4]) -> Result<(), ErrorReason> {
    if value
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
    {
        Ok(())
    } else {
        Err(ErrorReason::InvalidValue)
    }
}

fn positive(values: &[f32]) -> Result<(), ErrorReason> {
    if values.iter().all(|value| value.is_finite() && *value > 0.0) {
        Ok(())
    } else {
        Err(ErrorReason::InvalidValue)
    }
}

fn finite(value: f32) -> Result<(), ErrorReason> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ErrorReason::InvalidValue)
    }
}

fn fraction(value: f32) -> Result<(), ErrorReason> {
    if value.is_finite() && (0.0..1.0).contains(&value) {
        Ok(())
    } else {
        Err(ErrorReason::InvalidValue)
    }
}

impl PlotFrame2d {
    /// Local logical extent.
    pub fn size(&self) -> [f32; 2] {
        [self.width, self.height]
    }

    /// Local logical margins: left/top/right/bottom.
    pub fn padding(&self) -> [f32; 4] {
        [
            self.padding_left,
            self.padding_top,
            self.padding_right,
            self.padding_bottom,
        ]
    }

    /// Fixed lower bounds.
    pub fn min(&self) -> [f32; 2] {
        [self.min_x, self.min_y]
    }

    /// Fixed upper bounds.
    pub fn max(&self) -> [f32; 2] {
        [self.max_x, self.max_y]
    }
}

impl PlotFrame3d {
    /// Local physical axis extents.
    pub fn size(&self) -> [f32; 3] {
        [self.width, self.height, self.depth]
    }

    /// Fixed lower bounds.
    pub fn min(&self) -> [f32; 3] {
        [self.min_x, self.min_y, self.min_z]
    }

    /// Fixed upper bounds.
    pub fn max(&self) -> [f32; 3] {
        [self.max_x, self.max_y, self.max_z]
    }
}

macro_rules! frame_colors {
    ($name:ty) => {
        impl $name {
            /// Straight linear axis/text RGBA.
            pub fn color(&self) -> [f32; 4] {
                [self.red, self.green, self.blue, self.alpha]
            }

            /// Straight linear grid RGBA.
            pub fn grid_color(&self) -> [f32; 4] {
                [
                    self.grid_red,
                    self.grid_green,
                    self.grid_blue,
                    self.grid_alpha,
                ]
            }
        }
    };
}

frame_colors!(PlotFrame2d);
frame_colors!(PlotFrame3d);
