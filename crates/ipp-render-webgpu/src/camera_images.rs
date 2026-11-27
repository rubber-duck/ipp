//! Visual inputs for reusing a child Camera texture at identical projection and resolution.
use ipp_core::{
    HostRuntime, OutputRef, PublishedWorldAttachment, WorldPublicationId, WorldRef,
    services::asset_management::AssetLoadStatus,
    systems::{
        geometry::GeometrySystem,
        plot::{PlotPublication, PlotSystem},
        render::RenderSystem,
    },
};
use std::collections::BTreeMap;

use crate::device::Target;

#[derive(PartialEq)]
pub(super) struct CameraContent {
    world: WorldRef,
    selection: Option<OutputRef>,
    versions: [Option<u64>; 3],
    edges: Vec<PublishedWorldAttachment>,
}

pub(super) struct CameraImage {
    pub target: Target,
    pub frame: u64,
    pub content: Option<Vec<CameraContent>>,
    pub included: BTreeMap<OutputRef, WorldPublicationId>,
}

pub(super) fn collect(
    host: &HostRuntime,
    selection: Option<OutputRef>,
    id: WorldPublicationId,
    nodes: &mut Vec<CameraContent>,
    publications: &mut BTreeMap<OutputRef, WorldPublicationId>,
    depth: u32,
) -> Option<()> {
    if depth >= 64 {
        return None;
    }
    let world = host.publication(id)?;
    // Plot label transitions use presentation time even across identical World publications.
    if world
        .chunk(PlotSystem::ID)
        .and_then(|chunk| chunk.data::<PlotPublication>())
        .is_some_and(|plots| !plots.charts.is_empty())
    {
        return None;
    }
    for key in world.resources() {
        if !matches!(
            host.publication_resource(id, key)?.status(),
            AssetLoadStatus::Loaded
        ) {
            return None;
        }
    }
    let selected = match selection {
        Some(output) => {
            publications.insert(output, id);
            Some(host.output(id, output)?.version())
        }
        None => None,
    };
    let mut edges = Vec::with_capacity(world.attachments.len());
    for edge in &world.attachments {
        let Some(child) = host.attached_publication(edge) else {
            continue;
        };
        // An authored refresh interval can expire independently of World changes.
        if edge.surface_cache_policy.is_some() {
            return None;
        }
        let mut placement = edge.clone();
        placement.publication = None;
        edges.push(placement);
        collect(host, edge.output, child.id, nodes, publications, depth + 1)?;
    }
    nodes.push(CameraContent {
        world: world.world,
        selection,
        versions: [
            selected,
            world.chunk(RenderSystem::ID).map(|c| c.version()),
            world.chunk(GeometrySystem::ID).map(|c| c.version()),
        ],
        edges,
    });
    Some(())
}
