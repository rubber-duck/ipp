//! A Canvas paint revision that replaced some entries in place of the one the
//! RenderService last drew, through the RenderService with a recording device:
//! only the replaced boxes are hashed again, and a revision patched from one
//! the renderer did not draw hashes every box.

mod support;

use ipp_core::components::{CanvasBox, CanvasStyle};
use ipp_core::{Command, ComponentValue, EntityId, EntityRef, FieldValue, FieldWrite, HostRuntime};
use std::mem::offset_of;
use support::canvas::{CanvasSurface, add_content, apply};
use support::{render_frame, setup};

const BOXES: usize = 24;

fn shape(index: usize) -> Vec<ComponentValue> {
    vec![
        ComponentValue::CanvasStyle(CanvasStyle {
            x: (index % 6) as f32 * 0.15,
            y: (index / 6) as f32 * 0.15,
            ..Default::default()
        }),
        ComponentValue::CanvasBox(CanvasBox {
            width: 0.1,
            height: 0.1,
            ..Default::default()
        }),
    ]
}

fn tint(host: &mut HostRuntime, surface: &CanvasSurface, entity: EntityId, red: f32) {
    apply(
        host,
        surface.output.world().id(),
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CANVAS_STYLE,
            field: FieldWrite {
                offset: offset_of!(CanvasStyle, red) as u32,
                value: FieldValue::F32(red),
            },
        }],
    );
}

#[test]
fn a_patched_canvas_hashes_only_the_boxes_it_replaced() {
    let mut host = support::task_scheduler::host();
    let (context, mut renderer, _state) = setup(&mut host);
    let world = context.id();
    drop(context);
    let surface = CanvasSurface::new(&mut host, world, 0.0, shape(0));
    let boxes: Vec<EntityId> = std::iter::once(surface.content)
        .chain((1..BOXES).map(|index| add_content(&mut host, surface.output, shape(index))))
        .collect();
    let mut frame =
        |host: &mut HostRuntime| render_frame(&mut renderer, host, world, 100, 100).unwrap();
    let first = frame(&mut host);
    assert_eq!(
        first.gui_hashes as usize, BOXES,
        "the first frame hashes every box"
    );
    let warm = frame(&mut host);
    assert_eq!(warm.gui_hashes, 0, "an unchanged canvas hashes nothing");

    // Tinting one box replaces its entry in place of the drawn revision.
    tint(&mut host, &surface, boxes[7], 0.5);
    let tinted = frame(&mut host);
    let changes = surface
        .publication(&host)
        .paint_changes
        .clone()
        .expect("replaced in place");
    assert_eq!(changes.entries.len(), 1);
    assert_eq!(tinted.gui_hashes, 1, "only the tinted box is hashed");
    assert_eq!(tinted.gui_rebuilds, 1);
    assert_eq!(frame(&mut host).gui_hashes, 0);

    // Two patches between draws: the second was patched from a revision the
    // renderer never drew, so every box is hashed and only the tinted ones
    // rebuild.
    tint(&mut host, &surface, boxes[3], 0.25);
    tint(&mut host, &surface, boxes[11], 0.75);
    let skipped = frame(&mut host);
    assert_eq!(skipped.gui_hashes as usize, BOXES);
    assert_eq!(skipped.gui_rebuilds, 2);
}
