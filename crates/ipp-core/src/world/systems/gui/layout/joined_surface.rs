//! A joined popup and its anchor sample one optical sheet after native placement.

use super::entity_layout::GuiEntityLayout;
use super::overlay_placement::{GuiCanvasTransform, GuiOverlayLayout, canvas_transform};
use crate::EntityId;
use crate::world::WorldSimulationState;
use std::collections::BTreeMap;

pub(super) fn backgrounds(
    world: &WorldSimulationState,
    placements: &BTreeMap<EntityId, GuiEntityLayout>,
    overlays: &[GuiOverlayLayout],
) -> BTreeMap<EntityId, [f32; 4]> {
    let mut result = BTreeMap::new();
    for overlay in overlays {
        if !overlay.inputs.open
            || !world
                .components
                .gui_overlay(overlay.entity.index() as usize)
                .is_some_and(|value| value.join_anchor)
        {
            continue;
        }

        let Some(parent) = overlay.parent else {
            continue;
        };
        let (Some(anchor), Some(popup)) =
            (placements.get(&parent), placements.get(&overlay.entity))
        else {
            continue;
        };
        if !anchor.available || !popup.available || result.contains_key(&parent) {
            continue;
        }

        let a = canvas_transform(world, placements, parent);
        let b = canvas_transform(world, placements, overlay.entity);
        if let Some((anchor_bounds, popup_bounds)) = joined_rects(a, anchor.size, b, popup.size) {
            result.insert(parent, anchor_bounds);
            result.insert(overlay.entity, popup_bounds);
        }
    }
    result
}

fn joined_rects(
    a: GuiCanvasTransform,
    a_size: [f32; 2],
    b: GuiCanvasTransform,
    b_size: [f32; 2],
) -> Option<([f32; 4], [f32; 4])> {
    if a.scale != b.scale
        || a.scale.contains(&0.0)
        || a_size.iter().chain(b_size.iter()).any(|size| *size <= 0.0)
    {
        return None;
    }
    let aa = a.bounds(a_size);
    let bb = b.bounds(b_size);
    let near = |x: f32, y: f32| (x - y).abs() <= 1.0e-3;
    let vertical =
        near(aa[0], bb[0]) && near(aa[2], bb[2]) && (near(aa[3], bb[1]) || near(bb[3], aa[1]));
    let horizontal =
        near(aa[1], bb[1]) && near(aa[3], bb[3]) && (near(aa[2], bb[0]) || near(bb[2], aa[0]));
    if !vertical && !horizontal {
        return None;
    }
    let union = [
        aa[0].min(bb[0]),
        aa[1].min(bb[1]),
        aa[2].max(bb[2]),
        aa[3].max(bb[3]),
    ];
    let local = |transform: GuiCanvasTransform| {
        let mut rect = [0.0; 4];
        for axis in 0..2 {
            let lo = (union[axis] - transform.position[axis]) / transform.scale[axis];
            let hi = (union[axis + 2] - transform.position[axis]) / transform.scale[axis];
            rect[axis] = lo.min(hi);
            rect[axis + 2] = (hi - lo).abs();
        }
        rect
    };
    Some((local(a), local(b)))
}

#[cfg(test)]
#[path = "joined_surface_tests.rs"]
mod tests;
