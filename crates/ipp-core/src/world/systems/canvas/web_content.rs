//! Request reconciliation and publication image dependencies for CanvasHtml.

use super::{CanvasGeometry, CanvasPaintEntry, CanvasSystem, CanvasTarget};
use crate::services::web_content::{WebContentImageId, WebContentStatus, WebContentTarget};
use crate::systems::{SystemCommitContext, SystemNumericContext, SystemRuntimeAccess};
use crate::{ComponentValue, WorldRef};
use std::{collections::BTreeSet, sync::Arc};

#[cfg(test)]
#[path = "web_content_tests.rs"]
mod tests;

pub(super) fn image(entry: &CanvasPaintEntry) -> Option<WebContentImageId> {
    match entry {
        CanvasPaintEntry::Primitive {
            primitive,
            ..
        } => primitive.web_image(),
        CanvasPaintEntry::Attachment(_) => None,
    }
}

pub(super) fn images(entries: &[Arc<CanvasPaintEntry>]) -> BTreeSet<WebContentImageId> {
    entries.iter().filter_map(|entry| image(entry)).collect()
}

pub(super) fn retained_images(entries: &[Arc<CanvasPaintEntry>]) -> Arc<[WebContentImageId]> {
    images(entries).into_iter().collect::<Vec<_>>().into()
}

impl CanvasSystem {
    pub(super) fn prepare_web_content(&mut self, context: &mut SystemRuntimeAccess<'_>) {
        let layout = self
            .layout
            .and_then(|binding| context.dependency(binding))
            .and_then(|layout| layout.entity_view())
            .cloned();
        let world = &*context.world;
        let world_ref = WorldRef {
            id: world.id,
            incarnation: world.identity,
        };
        for &entity in &self.state.html_entities {
            let Some(html) = world.components.canvas_html(entity.index() as usize) else {
                continue;
            };
            let Some(incarnation) =
                super::update::incarnation(world, entity, ComponentValue::CANVAS_HTML)
            else {
                continue;
            };
            let canvas = CanvasTarget {
                entity,
                component: ComponentValue::CANVAS_HTML,
                incarnation,
            };
            let size = layout
                .as_ref()
                .and_then(|view| view.placements.get(&entity))
                .map_or([html.width, html.height], |placement| placement.size);
            let status = context.web_content.reconcile(
                WebContentTarget {
                    world: world_ref,
                    canvas,
                },
                html.source.clone(),
                size,
            );
            let image = match status {
                WebContentStatus::Ready(image) => Some(image),
                _ => None,
            };
            let before = self
                .state
                .leaves
                .get(&(entity, ComponentValue::CANVAS_HTML))
                .and_then(|leaf| leaf.geometry.as_ref());
            if !matches!(before, Some(CanvasGeometry::Html { image: old, .. }) if *old == image) {
                self.state
                    .geometry_dirty
                    .insert((entity, ComponentValue::CANVAS_HTML));
                self.state.patch.subtrees.insert(entity);
            }
        }
    }
}

pub(super) fn before_commit(context: &mut SystemCommitContext<'_>) {
    let world = WorldRef {
        id: context.world_data.id,
        incarnation: context.world_data.identity,
    };
    for (&(entity, component), &previous) in context.staged.changed.iter() {
        if component != ComponentValue::CANVAS_HTML {
            continue;
        }
        let Some(incarnation) = previous else {
            continue;
        };
        let target = WebContentTarget {
            world,
            canvas: CanvasTarget {
                entity,
                component,
                incarnation,
            },
        };
        if context.retains_component(entity, component) {
            context.web_content.cancel_request(target);
        } else {
            context.web_content.invalidate_target(target);
        }
    }
}

pub(super) fn before_numeric_update(context: &mut SystemNumericContext<'_>) {
    let world = WorldRef {
        id: context.world_data.id,
        incarnation: context.world_data.identity,
    };
    for &(entity, component) in context.changed {
        if component != ComponentValue::CANVAS_HTML {
            continue;
        }
        let Some(incarnation) = super::update::incarnation(context.world_data, entity, component)
        else {
            continue;
        };
        context.web_content.cancel_request(WebContentTarget {
            world,
            canvas: CanvasTarget {
                entity,
                component,
                incarnation,
            },
        });
    }
}
