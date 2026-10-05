//! Consume a ready completed binding and acknowledge only a complete successful rebuild.

use super::system_state::PlotRetainedChart;
use super::*;
use crate::systems::SystemUpdateContext;
use crate::systems::canvas::CanvasTarget;
use crate::systems::data_bindings::DataBindingAvailability;
use crate::{ComponentValue as C, EntityId, ErrorReason};
use std::sync::Arc;

pub(super) const CHARTS: [u16; 7] = [
    C::PLOT_LINE2D,
    C::PLOT_BARS2D,
    C::PLOT_PIE2D,
    C::PLOT_GRID_BARS3D,
    C::PLOT_HEIGHT_SURFACE3D,
    C::PLOT_POINTS3D,
    C::PLOT_PIE3D,
];

pub(super) fn is_chart(component: u16) -> bool {
    CHARTS.contains(&component)
}

pub(super) fn is_frame(component: u16) -> bool {
    matches!(component, C::PLOT_FRAME2D | C::PLOT_FRAME3D)
}

impl PlotSystem {
    pub(super) fn evaluate(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        if self.state.membership_dirty || self.state.members.is_empty() {
            self.state.members.clear();
            for (&entity, record) in &context.world.world.state.entities {
                for component in CHARTS {
                    if record.input(component).is_some() {
                        self.state.members.insert((entity, component));
                    }
                }
            }
            let stale: Vec<_> = self
                .state
                .charts
                .keys()
                .filter(|key| !self.state.members.contains(key))
                .copied()
                .collect();
            for key in stale {
                self.state.remove(key);
            }
            self.state.membership_dirty = false;
        }

        let members: Vec<_> = self.state.members.iter().copied().collect();
        for (entity, component) in members {
            if self
                .advance_interpolation(context, entity, component)
                .is_err()
            {
                self.state.remove((entity, component));
                continue;
            }
            let dirty = context.world.data_binding_dirty(entity).unwrap_or(true);
            if !dirty
                && !self.state.dirty.contains(&entity)
                && self.state.charts.contains_key(&(entity, component))
            {
                continue;
            }
            match self.prepare_chart(context, entity, component) {
                Ok(chart) => {
                    // The exact consumer lifetime is still current, and every output
                    // was successfully prepared, including requested fonts.
                    if context
                        .world
                        .finish_data_binding_presentation(chart.consumer)
                        .is_ok()
                    {
                        self.state.charts.insert((entity, component), chart);
                        self.state.changed();
                    } else {
                        self.state.remove((entity, component));
                    }
                }
                Err(_) => {
                    // Pending data/definition/font or invalid selectors suppress stale
                    // geometry and retain dirty for a later successful preparation.
                    self.state.remove((entity, component));
                }
            }
        }
        self.state.dirty.clear();
    }

    fn advance_interpolation(
        &self,
        context: &mut SystemUpdateContext<'_, '_>,
        entity: EntityId,
        component: u16,
    ) -> Result<(), ErrorReason> {
        let consumer = context
            .world
            .register_data_binding_presentation_consumer(entity, component)?;
        let Some(request) = context.data_binding_interpolation_request(consumer)? else {
            return Ok(());
        };
        let references = {
            let view = context.world.data_binding_prepared_view(entity)?;
            if !matches!(view.availability, DataBindingAvailability::Ready) {
                return Err(ErrorReason::InvalidValue);
            }
            let input = PlotPreparedInput {
                row_ids: view.row_ids,
                columns: view.columns,
            };
            super::interpolation_reference::references(
                &context.world.world.components,
                entity.index() as usize,
                component,
                &input,
                &request.outputs,
            )?
        };
        // The immutable pre-step borrow ends before the binding-owned operation
        // consumes this frame's Host delta. Geometry reads the advanced view.
        context
            .advance_data_binding_interpolation(consumer, &references)
            .map_err(|_| ErrorReason::InvalidValue)?;
        Ok(())
    }

    fn prepare_chart(
        &self,
        context: &mut SystemUpdateContext<'_, '_>,
        entity: EntityId,
        component: u16,
    ) -> Result<PlotRetainedChart, ErrorReason> {
        let consumer = context
            .world
            .register_data_binding_presentation_consumer(entity, component)?;
        let view = context.world.data_binding_prepared_view(entity)?;
        if !matches!(view.availability, DataBindingAvailability::Ready) {
            return Err(ErrorReason::InvalidValue);
        }
        let input = PlotPreparedInput {
            row_ids: view.row_ids,
            columns: view.columns,
        };
        let target = CanvasTarget {
            entity,
            component,
            incarnation: context.world.world.state.entities[&entity]
                .input(component)
                .ok_or(ErrorReason::MissingComponent)?
                .incarnation,
        };
        let storage = &context.world.world.components;
        let index = entity.index() as usize;
        let geometry = match component {
            C::PLOT_LINE2D => plots_2d::prepare_line(
                storage
                    .plot_line2d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                storage
                    .plot_frame2d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                &input,
            ),
            C::PLOT_BARS2D => plots_2d::prepare_bars(
                storage
                    .plot_bars2d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                storage
                    .plot_frame2d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                &input,
            ),
            C::PLOT_PIE2D => plots_2d::prepare_pie(
                storage
                    .plot_pie2d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                storage
                    .plot_frame2d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                &input,
            ),
            C::PLOT_GRID_BARS3D => plots_3d::prepare_bars(
                storage
                    .plot_grid_bars3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                storage
                    .plot_frame3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                &input,
            ),
            C::PLOT_HEIGHT_SURFACE3D => plots_3d::prepare_surface(
                storage
                    .plot_height_surface3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                storage
                    .plot_frame3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                &input,
            ),
            C::PLOT_POINTS3D => plots_3d::prepare_points(
                storage
                    .plot_points3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                storage
                    .plot_frame3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                &input,
            ),
            C::PLOT_PIE3D => plots_3d::prepare_pie(
                storage
                    .plot_pie3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                storage
                    .plot_frame3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                &input,
            ),
            _ => return Err(ErrorReason::InvalidValue),
        }?;
        let (source, variant) = if let Some(frame) = storage
            .plot_frame2d(index)
            .filter(|_| component <= C::PLOT_PIE2D)
        {
            (&frame.source, frame.variant)
        } else {
            let frame = storage
                .plot_frame3d(index)
                .ok_or(ErrorReason::MissingComponent)?;
            (&frame.source, frame.variant)
        };
        let assets = context.world.asset_resources();
        let font = if source.is_empty() {
            None
        } else {
            let key = assets
                .find_source(
                    context.world.world.id,
                    crate::services::asset_management::font::FONT_TYPE,
                    source,
                    variant,
                )
                .ok_or(ErrorReason::InvalidAsset)?;
            Some((
                key,
                assets
                    .get_typed::<crate::services::asset_management::font::FontAsset>(key)
                    .ok_or(ErrorReason::InvalidAsset)?,
            ))
        };
        let canvas = super::primitive_preparation::prepare(target, &geometry.canvas, font)?;
        let planes: Vec<_> = geometry
            .planes
            .iter()
            .map(|plane| {
                let primitives =
                    super::primitive_preparation::prepare(target, &plane.primitives, font)?;
                let bounds =
                    super::primitive_preparation::bounds(&primitives, font.map(|(_, font)| font));
                Ok(PlotPublishedPlane {
                    part: plane.part,
                    model: plane.model,
                    facing: plane.facing,
                    layout: plane.layout,
                    placement: plane.placement,
                    bounds,
                    clip: plane.clip,
                    primitives,
                })
            })
            .collect::<Result<_, ErrorReason>>()?;
        Ok(PlotRetainedChart {
            target,
            consumer,
            geometry: Arc::new(geometry),
            canvas,
            planes: planes.into(),
        })
    }
}
