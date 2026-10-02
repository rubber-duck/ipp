//! Borrowed Canvas presentation and exact nested-output dependencies.

use crate::RenderError;
use ipp_core::services::asset_management::{AssetKey, AssetProvider};
use ipp_core::systems::canvas::{
    CanvasAttachmentSlot, CanvasClip, CanvasPaintEntry, CanvasPrimitive, CanvasPrimitiveStyle,
    CanvasPublication,
};
use ipp_core::systems::{geometry::GeometrySystem, render::RenderSystem};
use ipp_core::{
    HostRuntime, OutputKind, OutputRef, PublishedWorldAttachment, WorldAttachmentMode,
    WorldAttachmentToken, WorldPublication, WorldPublicationId,
};

/// Exact source publication and ordered producer-owned paint; no copied primitive mirror.
#[derive(Clone, Copy)]
pub(super) struct CanvasScene<'a> {
    pub host: &'a HostRuntime,
    pub publication: &'a WorldPublication,
    pub canvas: &'a CanvasPublication,
}

impl<'a> CanvasScene<'a> {
    pub fn new(
        host: &'a HostRuntime,
        selection: OutputRef,
        publication: WorldPublicationId,
    ) -> Result<Self, RenderError> {
        let canvas = host
            .output(publication, selection)
            .and_then(|chunk| chunk.data::<CanvasPublication>())
            .filter(|canvas| canvas.selection == selection)
            .ok_or(RenderError::UnavailableOutput)?;
        let publication = host
            .publication(publication)
            .ok_or(RenderError::UnavailableOutput)?;

        Ok(Self {
            host,
            publication,
            canvas,
        })
    }

    pub fn primitive(&self, index: usize) -> Option<&'a CanvasPrimitive> {
        match self.canvas.entries.get(index)?.as_ref() {
            CanvasPaintEntry::Primitive {
                primitive,
                ..
            } => Some(primitive),
            CanvasPaintEntry::Attachment(_) => None,
        }
    }

    /// Whether this paint revision replaced the entry at `index` in place of
    /// the revision it patched; false when it patched none.
    pub fn replaced(&self, index: usize) -> bool {
        self.canvas
            .paint_changes
            .as_ref()
            .is_some_and(|changes| changes.replaced(index))
    }

    pub fn root_clip(&self) -> CanvasClip {
        [
            0.0,
            0.0,
            self.canvas.logical_extent[0],
            self.canvas.logical_extent[1],
        ]
    }

    pub fn resource(&self, key: AssetKey) -> Option<&'a AssetProvider> {
        self.host.publication_resource(self.publication.id, key)
    }

    /// A slot stays in painter order even when its exact child cannot be presented.
    pub fn attachment(&self, slot: &'a CanvasAttachmentSlot) -> CanvasAttachment<'a> {
        let edge = self.publication.attachments.iter().find(|edge| {
            edge.anchor == slot.anchor
                && edge.token == slot.token
                && edge.placement_output == Some(self.canvas.selection)
                && edge.surface_extent == Some(slot.physical_extent)
                && matches!(
                    edge.mode,
                    WorldAttachmentMode::SurfaceCanvas | WorldAttachmentMode::SurfaceCamera
                )
        });
        let child = edge.and_then(|edge| self.host.attached_publication(edge));
        CanvasAttachment {
            slot,
            edge,
            child,
        }
    }

    pub fn attachments(&self) -> impl Iterator<Item = CanvasAttachment<'a>> + '_ {
        self.canvas
            .entries
            .iter()
            .filter_map(|entry| match entry.as_ref() {
                CanvasPaintEntry::Attachment(slot) => Some(self.attachment(slot)),
                CanvasPaintEntry::Primitive {
                    ..
                } => None,
            })
    }
}

pub(super) struct CanvasAttachment<'a> {
    pub slot: &'a CanvasAttachmentSlot,
    pub edge: Option<&'a PublishedWorldAttachment>,
    pub child: Option<&'a WorldPublication>,
}

pub(super) fn effective_clip(
    style: &CanvasPrimitiveStyle,
    parent: CanvasClip,
) -> Option<CanvasClip> {
    let clip = [
        style.clip[0].max(parent[0]),
        style.clip[1].max(parent[1]),
        style.clip[2].min(parent[2]),
        style.clip[3].min(parent[3]),
    ];
    (style.opacity > 0.0 && !style.scale.contains(&0.0) && clip[0] < clip[2] && clip[1] < clip[3])
        .then_some(clip)
}

impl CanvasAttachment<'_> {
    pub fn output(&self) -> Option<(OutputRef, WorldPublicationId)> {
        Some((self.edge?.output?, self.child?.id))
    }

    /// Child logical coordinates to the containing Canvas, with no Hierarchy multiplication.
    pub fn child_mapping(&self, extent: [f32; 2], clip: CanvasClip) -> Option<CanvasMapping> {
        if extent
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return None;
        }
        let physical = self.slot.physical_extent;
        let scale = [
            self.slot.scale[0] * physical[0] / f64::from(extent[0]),
            -self.slot.scale[1] * physical[1] / f64::from(extent[1]),
        ];
        let origin = self.slot.to_canvas([-physical[0] * 0.5, physical[1] * 0.5]);
        CanvasMapping::new(origin, scale, clip, extent)
    }
}

/// One invertible logical mapping and the inherited clip expressed in child coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CanvasMapping {
    pub origin: [f64; 2],
    pub scale: [f64; 2],
    pub clip: CanvasClip,
}

impl CanvasMapping {
    pub fn new(
        origin: [f64; 2],
        scale: [f64; 2],
        clip: CanvasClip,
        extent: [f32; 2],
    ) -> Option<Self> {
        if origin.iter().chain(&scale).any(|value| !value.is_finite()) || scale.contains(&0.0) {
            return None;
        }
        if clip[0] >= clip[2] || clip[1] >= clip[3] {
            return Some(Self {
                origin,
                scale,
                clip: [0.0; 4],
            });
        }

        let inverse = |axis: usize, value: f32| (f64::from(value) - origin[axis]) / scale[axis];
        let bounds = [
            inverse(0, clip[0]),
            inverse(1, clip[1]),
            inverse(0, clip[2]),
            inverse(1, clip[3]),
        ];
        let clip = [
            bounds[0].min(bounds[2]).max(0.0) as f32,
            bounds[1].min(bounds[3]).max(0.0) as f32,
            bounds[0].max(bounds[2]).min(f64::from(extent[0])) as f32,
            bounds[1].max(bounds[3]).min(f64::from(extent[1])) as f32,
        ];
        Some(Self {
            origin,
            scale,
            clip,
        })
    }
}

/// All usable descendants, before their consumers, for graphics demand and target lifetime.
pub(super) fn output_order(
    host: &HostRuntime,
    selection: OutputRef,
    publication: WorldPublicationId,
) -> Result<Vec<(OutputRef, WorldPublicationId)>, RenderError> {
    if host.output(publication, selection).is_none() {
        return Err(RenderError::UnavailableOutput);
    }
    let mut order = Vec::new();
    let mut pending = vec![(selection, publication, false)];
    while let Some((selection, publication, visited)) = pending.pop() {
        if visited {
            order.push((selection, publication));
            continue;
        }
        let children = match selection.kind() {
            OutputKind::Canvas => CanvasScene::new(host, selection, publication)?
                .attachments()
                .filter(|attachment| attachment.slot.opacity > 0.0)
                .filter_map(|attachment| attachment.output())
                .collect::<Vec<_>>(),
            OutputKind::Camera => {
                let contributions = host
                    .spatial_contributions_for_output(publication, selection)
                    .map_err(|_| RenderError::UnavailableOutput)?;
                contributions
                    .iter()
                    .flat_map(|contribution| {
                        contribution
                            .publication
                            .attachments
                            .iter()
                            .filter_map(|edge| {
                                if edge.mode == WorldAttachmentMode::Spatial
                                    || edge
                                        .placement_output
                                        .is_some_and(|owner| owner != selection)
                                {
                                    return None;
                                }
                                Some((edge.output?, host.attached_publication(edge)?.id))
                            })
                    })
                    .collect()
            }
        };
        pending.push((selection, publication, true));
        pending.extend(
            children
                .into_iter()
                .rev()
                .map(|(selection, publication)| (selection, publication, false)),
        );
    }
    Ok(order)
}

/// Cache dependencies include unavailable slots; retaining parent paint alone is insufficient.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct OutputContentStamp {
    pub outputs: Vec<OutputNodeStamp>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct OutputNodeStamp {
    pub selection: OutputRef,
    pub visual: OutputVisualStamp,
    pub worlds: Vec<WorldContentStamp>,
    pub children: Vec<OutputChildStamp>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum OutputVisualStamp {
    Canvas {
        paint_revision: u64,
        /// Custom paint inputs, which change without changing the paint entries.
        paints_revision: u64,
        resource_revision: u64,
        logical_extent: [f32; 2],
        units_per_metre: f32,
    },
    Camera {
        version: u64,
    },
}

impl From<&CanvasPublication> for OutputVisualStamp {
    fn from(canvas: &CanvasPublication) -> Self {
        Self::Canvas {
            paint_revision: canvas.paint_revision,
            paints_revision: canvas.paints_revision,
            resource_revision: canvas.resource_revision,
            logical_extent: canvas.logical_extent,
            units_per_metre: canvas.units_per_metre,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct WorldContentStamp {
    pub world: ipp_core::WorldRef,
    pub render_version: Option<u64>,
    pub geometry_version: Option<u64>,
    pub resources: Vec<(AssetKey, bool)>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct OutputChildStamp {
    pub token: WorldAttachmentToken,
    pub placement: Option<PublishedPlacement>,
    pub available: bool,
    pub opacity: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PublishedPlacement {
    pub output: Option<OutputRef>,
    pub mode: WorldAttachmentMode,
    pub affine: [f64; 16],
    pub extent: Option<[f64; 2]>,
}

impl From<&PublishedWorldAttachment> for PublishedPlacement {
    fn from(edge: &PublishedWorldAttachment) -> Self {
        Self {
            output: edge.output,
            mode: edge.mode,
            affine: edge.placement,
            extent: edge.surface_extent,
        }
    }
}

impl OutputContentStamp {
    pub fn same_dependencies(&self, other: &Self) -> bool {
        self.outputs.len() == other.outputs.len()
            && self
                .outputs
                .iter()
                .zip(&other.outputs)
                .all(|(left, right)| {
                    left.selection == right.selection
                        && left.children == right.children
                        && left.worlds.len() == right.worlds.len()
                        && left.worlds.iter().zip(&right.worlds).all(|(left, right)| {
                            left.world == right.world && left.resources == right.resources
                        })
                })
    }

    pub fn contains_world(&self, world: ipp_core::WorldId) -> bool {
        self.outputs
            .iter()
            .any(|output| output.worlds.iter().any(|value| value.world.id() == world))
    }

    pub fn read(
        host: &HostRuntime,
        selection: OutputRef,
        publication: WorldPublicationId,
    ) -> Result<Self, RenderError> {
        let mut outputs = Vec::new();
        for (selection, publication) in output_order(host, selection, publication)? {
            let chunk = host
                .output(publication, selection)
                .ok_or(RenderError::UnavailableOutput)?;
            let mut stamp = OutputNodeStamp {
                selection,
                visual: match selection.kind() {
                    OutputKind::Canvas => OutputVisualStamp::from(
                        chunk
                            .data::<CanvasPublication>()
                            .ok_or(RenderError::UnavailableOutput)?,
                    ),
                    OutputKind::Camera => OutputVisualStamp::Camera {
                        version: chunk.version(),
                    },
                },
                worlds: Vec::new(),
                children: Vec::new(),
            };
            match selection.kind() {
                OutputKind::Canvas => {
                    let scene = CanvasScene::new(host, selection, publication)?;
                    stamp.worlds.push(WorldContentStamp {
                        world: scene.publication.world,
                        render_version: None,
                        geometry_version: None,
                        resources: scene
                            .canvas
                            .resources()
                            .map(|key| {
                                let ready = scene
                                    .resource(key)
                                    .and_then(|resource| resource.data())
                                    .is_some_and(|asset| asset.graphics_ready() != Some(false));
                                (key, ready)
                            })
                            .collect(),
                    });
                    for attachment in scene.attachments() {
                        stamp.children.push(OutputChildStamp {
                            token: attachment.slot.token.clone(),
                            placement: attachment.edge.map(PublishedPlacement::from),
                            available: attachment.child.is_some(),
                            opacity: attachment.slot.opacity,
                        });
                    }
                }
                OutputKind::Camera => {
                    for contribution in host
                        .spatial_contributions_for_output(publication, selection)
                        .map_err(|_| RenderError::UnavailableOutput)?
                    {
                        stamp
                            .worlds
                            .push(WorldContentStamp::read(host, contribution.publication));
                        for edge in &contribution.publication.attachments {
                            if edge
                                .placement_output
                                .is_some_and(|owner| owner != selection)
                            {
                                continue;
                            }
                            stamp.children.push(OutputChildStamp {
                                token: edge.token.clone(),
                                placement: Some(PublishedPlacement::from(edge)),
                                available: host.attached_publication(edge).is_some(),
                                opacity: 1.0,
                            });
                        }
                    }
                }
            }
            outputs.push(stamp);
        }
        Ok(Self {
            outputs,
        })
    }
}

impl WorldContentStamp {
    fn read(host: &HostRuntime, publication: &WorldPublication) -> Self {
        Self {
            world: publication.world,
            render_version: publication
                .chunk(RenderSystem::ID)
                .map(|chunk| chunk.version()),
            geometry_version: publication
                .chunk(GeometrySystem::ID)
                .map(|chunk| chunk.version()),
            resources: publication
                .resources()
                .map(|key| {
                    let ready = host
                        .publication_resource(publication.id, key)
                        .and_then(|resource| resource.data())
                        .is_some_and(|asset| asset.graphics_ready() != Some(false));
                    (key, ready)
                })
                .collect(),
        }
    }
}

#[cfg(test)]
#[path = "canvas_scene_tests.rs"]
mod tests;
