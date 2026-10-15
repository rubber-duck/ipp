//! Completed presentation inputs for paints, independent of retained local geometry.

use super::super::lighting::{lights::RenderLightingFrame, selection::surface_lighting};
use super::super::outputs::scene::{RenderEntity, RenderScene, SceneOutputSurface};
use super::scene::{CanvasScene, OutputContentStamp};
use crate::{RenderDevice, RenderError, RenderService};
use ipp_core::{HostRuntime, OutputRef, WorldPublicationId, math};
use std::sync::Arc;

#[derive(Clone)]
pub(in crate::services::render) struct CanvasPresentation {
    pub model: [f32; 16],
    pub content: [f32; 16],
    pub mapping: [f32; 3],
    pub spacing: f32,
    pub lighting: Arc<RenderLightingFrame>,
    pub selected: Arc<[RenderEntity]>,
    pub features: u32,
}

impl PartialEq for CanvasPresentation {
    fn eq(&self, other: &Self) -> bool {
        self.model == other.model
            && self.content == other.content
            && self.mapping == other.mapping
            && self.spacing == other.spacing
            && self.features == other.features
            && self.lighting.camera == other.lighting.camera
            && self.lighting.count == other.lighting.count
            && self.lighting.lights == other.lighting.lights
    }
}

impl CanvasPresentation {
    pub fn root(features: u32) -> Self {
        Self {
            model: super::layering::plane_matrix([0.0; 2], [1.0; 2]).expect("finite identity"),
            content: super::layering::plane_matrix([0.0; 2], [1.0, -1.0]).expect("finite root"),
            mapping: [0.0; 3],
            spacing: 0.0,
            lighting: Arc::new(RenderLightingFrame::empty([0.0, 0.0, 1.0, 1.0], [0.0; 3])),
            selected: Arc::from([]),
            features,
        }
    }

    pub fn surface(
        scene: &RenderScene<'_>,
        surface: &SceneOutputSurface,
        extent: [f32; 2],
        features: u32,
        previous: Option<&Self>,
    ) -> Result<Self, RenderError> {
        let geometry = &*surface.geometry;
        let (model, content, mapping) = if geometry.exact_affine(0.0).is_some() {
            let physical = geometry.physical_extent();
            let content = math::multiply(
                super::layering::affine_matrix(geometry)?,
                super::layering::plane_matrix(
                    [0.0; 2],
                    [
                        physical[0] / f64::from(extent[0]),
                        physical[1] / f64::from(extent[1]),
                    ],
                )?,
            );
            // Affine Surface front axes are retained in the model; Canvas +Y is down.
            let mut model = math::multiply(surface.model, content);
            for i in 0..3 {
                model[4 + i] = -model[4 + i];
            }
            (
                model,
                super::layering::plane_matrix([0.0; 2], [1.0, -1.0])?,
                [0.0; 3],
            )
        } else {
            let (kind, curvature) = if let Some(c) = geometry
                .as_any()
                .downcast_ref::<ipp_core::systems::surface::CylinderSurface>(
            ) {
                (1.0, c.curvature)
            } else if let Some(s) = geometry
                .as_any()
                .downcast_ref::<ipp_core::systems::surface::SphereSurface>()
            {
                (2.0, s.curvature)
            } else {
                return Err(RenderError::UnavailableOutput);
            };
            let [w, h] = geometry.physical_extent();
            (
                surface.model,
                super::layering::plane_matrix(
                    [-w * 0.5, h * 0.5],
                    [w / f64::from(extent[0]), -h / f64::from(extent[1])],
                )?,
                [kind, curvature, 0.0],
            )
        };
        let (lighting, selected) = surface_lighting(
            scene,
            surface,
            features & 8 != 0,
            previous.map_or(&[], |p| &p.selected),
        )?;
        Ok(Self {
            model,
            content,
            mapping,
            spacing: surface.layer_spacing,
            lighting: Arc::new(lighting),
            selected: selected.into(),
            features,
        })
    }

    pub fn layer(&self, coordinate: f64) -> Self {
        let mut result = self.clone();
        result.mapping[2] += (coordinate * f64::from(self.spacing)) as f32;
        result
    }

    pub fn child(
        &self,
        mapping: &super::scene::CanvasMapping,
        coordinate: f64,
        features: u32,
    ) -> Result<Self, RenderError> {
        let mut result = self.layer(coordinate);
        result.content = math::multiply(
            result.content,
            super::layering::plane_matrix(mapping.origin, mapping.scale)?,
        );
        result.spacing = 0.0;
        result.features = features;
        Ok(result)
    }

    pub fn upload<D: RenderDevice>(
        &self,
        device: &mut D,
        program: &D::Program,
    ) -> Result<(), RenderError> {
        device.set_lighting(
            program,
            &self.model,
            &self.content,
            &self.mapping,
            &self.lighting,
        )
    }
}

/// Exact output identities and immutable inputs represented by an image.
pub(in crate::services::render) type PaintContextStamp = Vec<(OutputRef, Arc<CanvasPresentation>)>;

impl<D: RenderDevice> RenderService<D> {
    pub(in crate::services::render) fn paint_context_stamp(
        &self,
        stamp: &OutputContentStamp,
    ) -> PaintContextStamp {
        stamp
            .outputs
            .iter()
            .filter_map(|node| {
                self.canvas_presentations
                    .get(&node.selection)
                    .filter(|context| context.features & 64 != 0)
                    .map(|context| (node.selection, context.clone()))
            })
            .collect()
    }
}

/// Gather once, child-first. Camera attachments start another presentation domain;
/// only Canvas children inherit their containing Surface's context requirements.
pub(in crate::services::render) fn output_features<D: RenderDevice>(
    host: &HostRuntime,
    selection: OutputRef,
    publication: WorldPublicationId,
) -> Result<std::collections::BTreeMap<OutputRef, u32>, RenderError> {
    let mut outputs = std::collections::BTreeMap::new();
    for (output, publication) in super::scene::output_order(host, selection, publication)? {
        if output.kind() != ipp_core::OutputKind::Canvas {
            continue;
        }
        let mut features = 0;
        let scene = CanvasScene::new(host, output, publication)?;
        for instance in scene.canvas.paints.iter() {
            let data = instance
                .shader
                .and_then(|key| scene.resource(key))
                .and_then(|resource| resource.data())
                .and_then(|asset| {
                    asset
                        .as_any()
                        .downcast_ref::<super::super::materials::shader_asset::GlShaderData<D>>()
                });
            if let Some(data) = data {
                features |= data.definition.recipe.features & (64 | 8);
            }
        }
        for child in scene
            .attachments()
            .filter(|attachment| attachment.slot.opacity > 0.0)
            .filter_map(|attachment| attachment.output())
        {
            features |= outputs.get(&child.0).copied().unwrap_or(0);
        }
        outputs.insert(output, features);
    }
    Ok(outputs)
}
