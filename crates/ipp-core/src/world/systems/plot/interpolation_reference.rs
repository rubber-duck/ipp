//! Cartesian percentage references use the same pre-step numeric fit as Plot.

use super::{PlotFrame2d, PlotFrame3d, PlotPreparedInput, PlotSeriesRow, plots_2d, plots_3d};
use crate::components::{registry::ComponentStorage, rows::Rows};
use crate::systems::data_bindings::DataBindingInterpolationReference;
use crate::{ComponentValue as C, DynamicPropertyKind, ErrorReason};

enum CartesianChart<'a> {
    Canvas {
        frame: &'a PlotFrame2d,
        series: &'a Rows<PlotSeriesRow>,
        bars: bool,
    },
    Scene {
        frame: &'a PlotFrame3d,
        series: &'a Rows<PlotSeriesRow>,
        bars: bool,
    },
}

impl<'a> CartesianChart<'a> {
    fn from_storage(
        storage: &'a ComponentStorage,
        index: usize,
        component: u16,
    ) -> Result<Self, ErrorReason> {
        match component {
            C::PLOT_LINE2D | C::PLOT_BARS2D => Ok(Self::Canvas {
                frame: storage
                    .plot_frame2d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                series: if component == C::PLOT_BARS2D {
                    &storage
                        .plot_bars2d(index)
                        .ok_or(ErrorReason::MissingComponent)?
                        .series
                } else {
                    &storage
                        .plot_line2d(index)
                        .ok_or(ErrorReason::MissingComponent)?
                        .series
                },
                bars: component == C::PLOT_BARS2D,
            }),
            C::PLOT_GRID_BARS3D | C::PLOT_HEIGHT_SURFACE3D | C::PLOT_POINTS3D => Ok(Self::Scene {
                frame: storage
                    .plot_frame3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                series: match component {
                    C::PLOT_GRID_BARS3D => {
                        &storage
                            .plot_grid_bars3d(index)
                            .ok_or(ErrorReason::MissingComponent)?
                            .series
                    }
                    C::PLOT_HEIGHT_SURFACE3D => {
                        &storage
                            .plot_height_surface3d(index)
                            .ok_or(ErrorReason::MissingComponent)?
                            .series
                    }
                    _ => {
                        &storage
                            .plot_points3d(index)
                            .ok_or(ErrorReason::MissingComponent)?
                            .series
                    }
                },
                bars: component == C::PLOT_GRID_BARS3D,
            }),
            // Pie shares, physical radius/height and colour are not Cartesian axes.
            _ => Err(ErrorReason::InvalidField),
        }
    }

    fn axis(&self, output: &str) -> Result<usize, ErrorReason> {
        let (series, dimensions, value_y) = match self {
            Self::Canvas {
                series,
                ..
            } => (*series, 2, false),
            Self::Scene {
                series,
                bars,
                ..
            } => (*series, 3, *bars),
        };
        let mut axis = None;
        // Association follows authored selectors, including temporarily hidden
        // series; the numeric fit itself uses visible valid samples only.
        for (_, row) in series.iter() {
            let selectors = [
                row.x.as_ref(),
                if value_y {
                    row.value.as_ref()
                } else {
                    row.y.as_ref()
                },
                row.z.as_ref(),
            ];
            for (candidate, selector) in selectors[..dimensions].iter().enumerate() {
                if !selector.is_empty() && *selector == output {
                    if axis.is_some_and(|axis| axis != candidate) {
                        return Err(ErrorReason::InvalidValue);
                    }
                    axis = Some(candidate);
                }
            }
        }
        axis.ok_or(ErrorReason::InvalidField)
    }

    fn ranges(&self, input: &PlotPreparedInput<'_>) -> Result<([f64; 3], [f64; 3]), ErrorReason> {
        match self {
            Self::Canvas {
                frame,
                series,
                bars,
            } => {
                let (min, max) = plots_2d::interpolation_ranges(frame, series, input, *bars)?;
                Ok(([min[0], min[1], 0.0], [max[0], max[1], 0.0]))
            }
            Self::Scene {
                frame,
                series,
                bars,
            } => plots_3d::interpolation_ranges(frame, series, input, *bars),
        }
    }

    fn references<'b>(
        &self,
        input: &PlotPreparedInput<'_>,
        outputs: &'b [String],
    ) -> Result<Vec<DataBindingInterpolationReference<'b>>, ErrorReason> {
        let axes: Vec<_> = outputs
            .iter()
            .map(|output| {
                if input.bind(output)?.kind != DynamicPropertyKind::F32 {
                    return Err(ErrorReason::InvalidValue);
                }
                self.axis(output)
            })
            .collect::<Result<_, _>>()?;
        let (min, max) = self.ranges(input)?;
        Ok(outputs
            .iter()
            .zip(axes)
            .map(|(output, axis)| DataBindingInterpolationReference {
                output,
                maximum: min[axis].abs().max(max[axis].abs()),
            })
            .collect())
    }
}

pub(super) fn references<'a>(
    storage: &ComponentStorage,
    index: usize,
    component: u16,
    input: &PlotPreparedInput<'_>,
    outputs: &'a [String],
) -> Result<Vec<DataBindingInterpolationReference<'a>>, ErrorReason> {
    CartesianChart::from_storage(storage, index, component)?.references(input, outputs)
}

#[cfg(test)]
#[path = "interpolation_reference_tests.rs"]
mod tests;
