use super::WorldAttachmentSystem;
use crate::host::attachments::topology::AttachmentAnchor;
use crate::{AttachmentPlacement, PublishedWorldAttachment, WorldContext, systems};

impl WorldAttachmentSystem {
    pub(super) fn collect_attachments(
        &self,
        world: &WorldContext<'_>,
        output: &mut Vec<PublishedWorldAttachment>,
    ) {
        for &entity in world.world.state.entities.keys() {
            let Some(value) = world
                .world
                .components
                .world_attachment(entity.index() as usize)
            else {
                continue;
            };
            let Some(child) = value.child() else {
                continue;
            };
            if world.topology.worlds.get(&child.id) != Some(&child) {
                continue;
            }
            let (placement_output, placement) = match world.attachment_placement(entity) {
                Ok(AttachmentPlacement::Ready {
                    owner,
                    affine,
                }) => (Some(owner), affine),
                Ok(AttachmentPlacement::Unavailable {
                    ..
                })
                | Err(_) => continue,
                Ok(AttachmentPlacement::Unmanaged) => {
                    let placement = if world
                        .manifest()
                        .systems()
                        .contains(&systems::hierarchy::HierarchySystem::ID)
                    {
                        let Ok(placement) =
                            systems::hierarchy::evaluated_affine(world.world, entity)
                        else {
                            continue;
                        };
                        placement.matrix()
                    } else {
                        crate::host::references::IDENTITY
                    };
                    (None, placement)
                }
            };
            let surface = (value.mode != 0).then(|| world.surface(entity)).flatten();
            let surface_extent = surface.as_ref().map(|surface| surface.physical_extent());
            let surface_cache_policy = surface_extent
                .and_then(|_| {
                    world
                        .world
                        .components
                        .surface_cache(entity.index() as usize)
                })
                .and_then(|cache| systems::surface::SurfaceCachePolicy::new(cache).ok());
            output.push(PublishedWorldAttachment {
                token: world.topology.tokens[&AttachmentAnchor {
                    world: world.world.id,
                    entity,
                }]
                    .clone(),
                anchor: entity,
                child,
                mode: value.mode(),
                output: value.presented_output(),
                placement_output,
                placement,
                surface_extent,
                surface_incarnation: surface
                    .as_ref()
                    .and_then(|surface| world.component_incarnation(entity, surface.component())),
                surface_geometry: surface.clone(),
                surface_cache_policy,
                layer_spacing: surface
                    .as_ref()
                    .map_or(0.0, |surface| surface.layer_spacing()),
                publication: None,
            });
        }
    }
}
