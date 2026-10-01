use super::WorldAttachmentSystem;
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
                        crate::host::attachment::IDENTITY
                    };
                    (None, placement)
                }
            };
            let surface_extent = (value.mode != 0)
                .then(|| world.world.components.surface(entity.index() as usize))
                .flatten()
                .map(|surface| [f64::from(surface.width), f64::from(surface.height)]);
            let surface_cache_policy = surface_extent
                .and_then(|_| {
                    world
                        .world
                        .components
                        .surface_cache(entity.index() as usize)
                })
                .and_then(|cache| systems::surface::SurfaceCachePolicy::new(cache).ok());
            output.push(PublishedWorldAttachment {
                token: world.topology.tokens[&crate::host::topology::AttachmentAnchor {
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
                surface_cache_policy,
                publication: None,
            });
        }
    }
}
