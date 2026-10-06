//! Ordinary GuiLayout producers and immutable Canvas consumers on the maintained EGL driver.

use super::canvas_publications::{canvas, frame_at};
use super::publications::{apply, assert_color, create, save};
use ipp_core::components::{CanvasBox, CanvasStyle, CanvasText, FlatSurface, GuiLayout};
use ipp_core::services::asset_management::{AssetSource, formats::font::FONT_TYPE};
use ipp_core::systems::canvas::{CanvasPaintEntry, CanvasPrimitive, CanvasPublication};
use ipp_core::{
    Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef, HostRuntime, OutputRef,
    WorldAttachment, WorldId, WorldViewport,
};
use ipp_render_gl::{RenderDevice, RenderService};
use std::{path::Path, sync::Arc};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn append(
    host: &mut HostRuntime,
    world: WorldId,
    parent: EntityId,
    values: Vec<ComponentValue>,
) -> Result<EntityId> {
    let entity = create(host, world, values)?;
    apply(
        host,
        world,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(entity),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        }],
    )?;
    Ok(entity)
}

fn snapshot(host: &HostRuntime, output: OutputRef) -> CanvasPublication {
    let publication = host.latest_publication(output.world().id()).unwrap();
    host.output(publication, output)
        .unwrap()
        .data::<CanvasPublication>()
        .unwrap()
        .clone()
}

fn primitive(canvas: &CanvasPublication, entity: EntityId) -> &CanvasPrimitive {
    canvas
        .entries
        .iter()
        .find_map(|entry| match entry.as_ref() {
            CanvasPaintEntry::Primitive {
                primitive,
                ..
            } if primitive.style().identity.target.entity == entity => Some(primitive),
            _ => None,
        })
        .expect("published ordinary leaf")
}

fn shape(
    host: &mut HostRuntime,
    world: WorldId,
    parent: EntityId,
    size: [f32; 2],
    color: [f32; 3],
) -> Result<EntityId> {
    append(
        host,
        world,
        parent,
        vec![
            ComponentValue::CanvasBox(CanvasBox {
                width: 1.0,
                height: 1.0,
                ..Default::default()
            }),
            ComponentValue::GuiLayout(GuiLayout {
                width: size[0],
                height: size[1],
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                red: color[0],
                green: color[1],
                blue: color[2],
                ..Default::default()
            }),
        ],
    )
}

fn assert_values<const COUNT: usize>(actual: [f32; COUNT], expected: [f32; COUNT]) {
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() < 1e-4),
        "{actual:?} != {expected:?}"
    );
}

fn yellow_pixels(pixels: &[u8], bounds: [usize; 4]) -> usize {
    (bounds[1]..bounds[3])
        .flat_map(|row| (bounds[0]..bounds[2]).map(move |column| (row * 256 + column) * 4))
        .filter(|offset| {
            pixels[*offset] > 150 && pixels[*offset + 1] > 150 && pixels[*offset + 2] < 40
        })
        .count()
}

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
    fonts: &Path,
) -> Result<()> {
    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let parent = host.create_world(Default::default(), &super::selection::panel())?;
    let child = host.create_world(Default::default(), &super::selection::panel())?;
    let root = canvas(&mut host, parent, 100.0)?;
    let nested = canvas(&mut host, child, 80.0)?;
    create(
        &mut host,
        parent,
        vec![
            ComponentValue::CanvasBox(CanvasBox {
                width: 128.0,
                height: 128.0,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                ..Default::default()
            }),
        ],
    )?;
    let column = create(
        &mut host,
        parent,
        vec![
            ComponentValue::GuiLayout(GuiLayout {
                kind: 2,
                width: 112.0,
                height: 112.0,
                padding_left: 8.0,
                padding_top: 8.0,
                clip: true,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 8.0,
                y: 8.0,
                ..Default::default()
            }),
        ],
    )?;
    let row = append(
        &mut host,
        parent,
        column,
        vec![ComponentValue::GuiLayout(GuiLayout {
            kind: 1,
            width: 96.0,
            height: 28.0,
            padding_left: 4.0,
            padding_top: 2.0,
            clip: true,
            ..Default::default()
        })],
    )?;
    let red = shape(&mut host, parent, row, [24.0, 20.0], [0.5, 0.0, 0.0])?;
    let font = AssetSource {
        kind: FONT_TYPE,
        uri: format!("producer://{}/17/100", parent.0).into(),
        variant: 0,
    };
    let bytes = std::fs::read(fonts.join("shure-tech-mono.ippf"))?;
    let progress_limit = bytes
        .len()
        .div_ceil(ipp_core::services::asset_management::STREAM_CAPACITY)
        + 32;
    host.asset_resources_mut()
        .register_client_source(parent, font.clone(), bytes)?;
    let font_key = host.asset_resources().find(&font).unwrap();
    let text = append(
        &mut host,
        parent,
        row,
        vec![
            ComponentValue::CanvasText(CanvasText {
                text: "OO".into(),
                source: font.uri,
                font_size: 16.0,
                ..Default::default()
            }),
            ComponentValue::GuiLayout(GuiLayout {
                width: 64.0,
                height: 20.0,
                padding_left: 2.0,
                padding_top: 1.0,
                clip: true,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                red: 1.0,
                green: 1.0,
                blue: 0.0,
                ..Default::default()
            }),
        ],
    )?;
    let surface = FlatSurface {
        width: 1.0,
        height: 0.5,
        ..Default::default()
    };
    let anchor = append(
        &mut host,
        parent,
        column,
        vec![
            ComponentValue::FlatSurface(surface),
            ComponentValue::WorldAttachment(WorldAttachment::surface(nested)),
            ComponentValue::GuiLayout(GuiLayout {
                width: 64.0,
                height: 48.0,
                padding_left: 4.0,
                padding_top: 4.0,
                clip: true,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 4.0,
                y: 4.0,
                scale_x: 1.25,
                ..Default::default()
            }),
        ],
    )?;
    let panel = create(
        &mut host,
        child,
        vec![ComponentValue::GuiLayout(GuiLayout {
            kind: 1,
            padding_left: 8.0,
            padding_top: 4.0,
            clip: true,
            ..Default::default()
        })],
    )?;
    let green = shape(&mut host, child, panel, [24.0, 24.0], [0.0, 0.5, 0.0])?;
    let blue = shape(&mut host, child, panel, [48.0, 36.0], [0.0, 0.0, 0.5])?;
    let viewport = WorldViewport {
        width: 256,
        height: 256,
        device_pixel_ratio: 2.0,
    };
    host.set_root_output(root, viewport)?;

    for _ in 0..progress_limit {
        host.frame(0.0)?;
        let selected = host
            .root_output(root.world().id())
            .map(|(output, _, publication)| (output, publication));
        renderer.prepare(&mut host, selected)?;
        host.progress_assets();
        if host
            .asset_resources()
            .get(font_key)
            .and_then(|resource| resource.data())
            .is_some_and(|data| data.graphics_ready() != Some(false))
        {
            break;
        }
    }
    assert!(
        host.asset_resources()
            .get(font_key)
            .and_then(|resource| resource.data())
            .is_some_and(|data| data.graphics_ready() != Some(false))
    );
    frame_at(renderer, &mut host, root, viewport, 0.0)?;
    let initial = snapshot(&host, root);
    let child_initial = snapshot(&host, nested);
    assert_eq!(initial.logical_extent, [128.0, 128.0]);
    assert_eq!(child_initial.logical_extent, [80.0, 40.0]);
    assert_values(primitive(&initial, red).style().position, [20.0, 18.0]);
    assert_values(
        primitive(&initial, red).style().clip,
        [16.0, 16.0, 112.0, 44.0],
    );
    let CanvasPrimitive::Box {
        size,
        ..
    } = primitive(&initial, red)
    else {
        panic!("box");
    };
    assert_eq!(*size, [24.0, 20.0]);
    assert_values(primitive(&initial, text).style().position, [46.0, 19.0]);
    let CanvasPrimitive::Glyphs {
        glyphs: original_glyphs,
        ..
    } = primitive(&initial, text)
    else {
        panic!("glyphs");
    };
    assert_eq!(original_glyphs.len(), 2);
    let original_glyphs = original_glyphs.clone();
    assert_values(
        primitive(&child_initial, green).style().position,
        [8.0, 4.0],
    );
    assert_values(
        primitive(&child_initial, blue).style().position,
        [32.0, 4.0],
    );

    let slot = initial
        .entries
        .iter()
        .find_map(|entry| match entry.as_ref() {
            CanvasPaintEntry::Attachment(slot) if slot.anchor == anchor => Some(slot),
            _ => None,
        })
        .unwrap();
    assert_eq!(slot.physical_extent, [1.0, 0.5]);
    assert_eq!(slot.to_canvas([-0.5, 0.25]), [25.0, 52.0]);
    assert_eq!(slot.to_canvas([0.5, -0.25]), [105.0, 100.0]);
    assert_values(slot.clip, [20.0, 48.0, 100.0, 96.0]);
    let hit = initial
        .hits
        .iter()
        .find(|hit| hit.target.entity == anchor)
        .unwrap();
    assert_values(hit.bounds, [25.0, 52.0, 105.0, 100.0]);
    assert_eq!(hit.clip, slot.clip);
    assert!(hit.contains([98.0, 80.0]));
    assert!(!hit.contains([102.0, 80.0]));
    let publication = host.latest_publication(parent).unwrap();
    let edge = host
        .publication(publication)
        .unwrap()
        .attachments
        .iter()
        .find(|edge| edge.anchor == anchor)
        .unwrap();
    assert_eq!(edge.placement_output, Some(root));
    assert_eq!(edge.token, slot.token);
    assert_eq!(edge.surface_extent, Some([1.0, 0.5]));
    assert_eq!(
        edge.placement,
        slot.parent_affine(initial.logical_extent, initial.units_per_metre)
    );

    {
        let world = host.world_mut(parent).unwrap();
        assert_values(world.gui_entity_layout(row).unwrap().origin, [8.0, 8.0]);
        assert_values(world.gui_entity_layout(red).unwrap().origin, [4.0, 2.0]);
        assert_values(world.gui_entity_layout(text).unwrap().origin, [28.0, 2.0]);
        assert_values(world.gui_entity_layout(anchor).unwrap().origin, [8.0, 36.0]);
        assert_values(world.gui_entity_layout(anchor).unwrap().size, [64.0, 48.0]);
        assert!(world.gui_entity_layout_diagnostics().unwrap().is_empty());
    }
    let first = capture()?;
    save(output, "gui-ordinary-layout-initial", &first)?;
    for (point, color) in [
        ([50, 44], [0.5, 0.0, 0.0]),
        ([36, 44], [0.0; 3]),
        ([80, 140], [0.0, 0.5, 0.0]),
        ([160, 150], [0.0, 0.0, 0.5]),
        ([196, 180], [0.0, 0.0, 0.5]),
        ([204, 180], [0.0; 3]),
        ([180, 188], [0.0, 0.0, 0.5]),
        ([180, 198], [0.0; 3]),
    ] {
        assert_color(&first, point[0], point[1], color);
    }
    let first_ink = yellow_pixels(&first, [90, 30, 224, 80]);
    assert!(
        first_ink > 100,
        "ordinary measured text has no visible contours: {first_ink}"
    );

    for _ in 0..4 {
        frame_at(renderer, &mut host, root, viewport, 0.0)?;
    }
    let warm = capture()?;
    let warm_parent = host
        .world_mut(parent)
        .unwrap()
        .gui_entity_layout_statistics()
        .unwrap();
    let warm_child = host
        .world_mut(child)
        .unwrap()
        .gui_entity_layout_statistics()
        .unwrap();
    frame_at(renderer, &mut host, root, viewport, 0.0)?;
    assert_eq!(renderer.statistics().gui_rebuilds, 0);
    assert_eq!(renderer.statistics().uploaded_bytes, 0);
    assert_eq!(capture()?, warm);
    assert!(Arc::ptr_eq(
        &initial.entries,
        &snapshot(&host, root).entries
    ));

    apply(
        &mut host,
        parent,
        vec![Command::SetField {
            entity: EntityRef::Handle(column),
            component: ComponentValue::CANVAS_STYLE,
            field: ipp_core::FieldWrite {
                offset: std::mem::offset_of!(CanvasStyle, x) as u32,
                value: ipp_core::FieldValue::F32(16.0),
            },
        }],
    )?;
    frame_at(renderer, &mut host, root, viewport, 1.0)?;
    let moved = snapshot(&host, root);
    let CanvasPrimitive::Glyphs {
        glyphs,
        ..
    } = primitive(&moved, text)
    else {
        panic!("glyphs");
    };
    assert!(Arc::ptr_eq(&original_glyphs, glyphs));
    assert_values(primitive(&moved, red).style().position, [28.0, 18.0]);
    assert_eq!(
        host.world_mut(parent)
            .unwrap()
            .gui_entity_layout_statistics()
            .unwrap()
            .total,
        warm_parent.total
    );
    assert_eq!(
        host.world_mut(child)
            .unwrap()
            .gui_entity_layout_statistics()
            .unwrap()
            .total,
        warm_child.total
    );
    let moved_pixels = capture()?;
    save(output, "gui-ordinary-layout-visual", &moved_pixels)?;
    assert_color(&moved_pixels, 42, 44, [0.0; 3]);
    assert_color(&moved_pixels, 66, 44, [0.5, 0.0, 0.0]);
    assert_color(&moved_pixels, 212, 180, [0.0, 0.0, 0.5]);
    assert_color(&moved_pixels, 220, 180, [0.0; 3]);

    apply(
        &mut host,
        parent,
        vec![Command::SetField {
            entity: EntityRef::Handle(text),
            component: ComponentValue::CANVAS_TEXT,
            field: ipp_core::FieldWrite {
                offset: std::mem::offset_of!(CanvasText, text) as u32,
                value: ipp_core::FieldValue::String("OOOO".into()),
            },
        }],
    )?;
    frame_at(renderer, &mut host, root, viewport, 2.0)?;
    let changed = snapshot(&host, root);
    let CanvasPrimitive::Glyphs {
        glyphs,
        ..
    } = primitive(&changed, text)
    else {
        panic!("glyphs");
    };
    assert_eq!(glyphs.len(), 4);
    assert!(!Arc::ptr_eq(&original_glyphs, glyphs));
    assert_eq!(
        primitive(&changed, text).style().identity,
        primitive(&initial, text).style().identity
    );
    assert_values(primitive(&changed, text).style().position, [54.0, 19.0]);
    let changed_work = host
        .world_mut(parent)
        .unwrap()
        .gui_entity_layout_statistics()
        .unwrap();
    assert_eq!(changed_work.total.reflows, warm_parent.total.reflows + 1);
    assert_eq!(
        changed_work.total.text_measurements,
        warm_parent.total.text_measurements + 1
    );
    assert_eq!(
        host.world_mut(child)
            .unwrap()
            .gui_entity_layout_statistics()
            .unwrap()
            .total,
        warm_child.total
    );
    let changed_pixels = capture()?;
    save(output, "gui-ordinary-layout-text-changed", &changed_pixels)?;
    let changed_ink = yellow_pixels(&changed_pixels, [106, 30, 240, 80]);
    assert!(
        changed_ink > first_ink * 3 / 2,
        "text edit did not produce new glyph contours: {first_ink} -> {changed_ink}"
    );
    assert_color(&changed_pixels, 66, 44, [0.5, 0.0, 0.0]);
    assert_color(&changed_pixels, 212, 180, [0.0, 0.0, 0.5]);
    frame_at(renderer, &mut host, root, viewport, 2.0)?;
    assert_eq!(renderer.statistics().gui_rebuilds, 0);
    assert_eq!(renderer.statistics().uploaded_bytes, 0);
    assert_eq!(capture()?, changed_pixels);
    std::fs::write(
        output.join("gui-ordinary-layout.txt"),
        format!(
            "root={:?}\nchild_extent={:?}\nslot_extent={:?}\nslot_clip={:?}\ninitial_glyphs=2\nchanged_glyphs=4\nink={first_ink}->{changed_ink}\nwarm_layout={warm_parent:?}\nchanged_layout={changed_work:?}\n",
            initial.selection, child_initial.logical_extent, slot.physical_extent, slot.clip,
        ),
    )?;
    renderer.prepare(&mut host, None)?;
    renderer.unload_host(&mut host)?;
    Ok(())
}
