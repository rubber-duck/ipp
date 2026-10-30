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
    matches!(
        component,
        C::PLOT_FRAME2D | C::PLOT_FRAME3D | C::PLOT_CALLOUT_STYLE
    )
}

impl PlotSystem {
    pub(super) fn evaluate(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        if self.state.visibility_dirty {
            let mut eligibility = crate::systems::hierarchy::PresentationEligibility::default();
            self.state.visible_members.clear();
            self.state
                .visible_members
                .extend(
                    self.state
                        .members
                        .iter()
                        .enumerate()
                        .filter_map(|(index, member)| {
                            eligibility
                                .open(context.world.world, member.key.0)
                                .then_some(index)
                        }),
                );
            self.state.visibility_dirty = false;
        }
        // Hidden branches retain dirty inputs without polling bindings or preparing
        // charts. Membership, gate and effective-link edits rebuild this selection.
        for visible in 0..self.state.visible_members.len() {
            let index = self.state.visible_members[visible];
            let member = self.state.members[index];
            let (entity, component) = member.key;
            if !member.dirty
                && !context.world.data_binding_dirty(entity).unwrap_or(true)
                && self.state.charts.contains_key(&member.key)
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
                        self.state.members[index].dirty = false;
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
                storage.plot_callout_style(index),
            ),
            C::PLOT_BARS2D => plots_2d::prepare_bars(
                storage
                    .plot_bars2d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                storage
                    .plot_frame2d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                &input,
                storage.plot_callout_style(index),
            ),
            C::PLOT_PIE2D => plots_2d::prepare_pie(
                storage
                    .plot_pie2d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                storage
                    .plot_frame2d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                &input,
                storage.plot_callout_style(index),
            ),
            C::PLOT_GRID_BARS3D => plots_3d::prepare_bars(
                storage
                    .plot_grid_bars3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                storage
                    .plot_frame3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                &input,
                storage.plot_callout_style(index),
            ),
            C::PLOT_HEIGHT_SURFACE3D => plots_3d::prepare_surface(
                storage
                    .plot_height_surface3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                storage
                    .plot_frame3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                &input,
                storage.plot_callout_style(index),
            ),
            C::PLOT_POINTS3D => plots_3d::prepare_points(
                storage
                    .plot_points3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                storage
                    .plot_frame3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                &input,
                storage.plot_callout_style(index),
            ),
            C::PLOT_PIE3D => plots_3d::prepare_pie(
                storage
                    .plot_pie3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                storage
                    .plot_frame3d(index)
                    .ok_or(ErrorReason::MissingComponent)?,
                &input,
                storage.plot_callout_style(index),
            ),
            _ => return Err(ErrorReason::InvalidValue),
        }?;

        // A dirty binding or component can produce the same complete local
        // result. Keep its allocation identity after one comparison here, so
        // publication and renderer caches do not repeatedly compare or upload
        // equal replacement streams on otherwise unchanged frames.
        let geometry = match self.state.charts.get(&(entity, component)) {
            Some(previous) if previous.target == target && *previous.geometry == geometry => {
                previous.geometry.clone()
            }
            _ => Arc::new(geometry),
        };

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
                    crate::services::asset_management::formats::font::FONT_TYPE,
                    source,
                    variant,
                )
                .ok_or(ErrorReason::InvalidAsset)?;
            Some((
                key,
                assets
                    .get_typed::<crate::services::asset_management::formats::font::FontAsset>(key)
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
            geometry,
            canvas,
            planes: planes.into(),
        })
    }
}
