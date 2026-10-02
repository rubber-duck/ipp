//! Ordinary control paint and same-publication hit/value observations on real GLES.

use super::canvas_publications::{canvas, frame_at, place};
use super::publications::{apply, assert_color, create, save};
use ipp_core::components::rows::Rows;
use ipp_core::components::{
    CanvasBox, CanvasStyle, GuiBehavior, GuiButton, GuiCheckbox, GuiLayout, GuiSlider, GuiTextInput,
};
use ipp_core::services::asset_management::{AssetSource, font::FONT_TYPE};
use ipp_core::systems::canvas::{
    CanvasGlyph, CanvasPaintEntry, CanvasPart, CanvasPrimitive, CanvasPublication, CanvasSystem,
};
use ipp_core::systems::gui::GuiPrimitivePart;
use ipp_core::systems::gui::local::GuiControlKind;
use ipp_core::systems::gui::presentation::{
    GuiCanvasPublication, GuiControlObservation, GuiFont, GuiPaintPart, GuiRoutingValue, GuiSkin,
    GuiTheme,
};
use ipp_core::systems::gui::{GuiPartId, GuiPartVariant, GuiSkinState};
use ipp_core::{
    Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef, FieldValue, FieldWrite,
    HostRuntime, OutputRef, WorldViewport,
};
use ipp_render_gl::{RenderDevice, RenderService};
use std::{mem::offset_of, path::Path, sync::Arc};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const GREY: [f32; 3] = [0.125; 3];
const GREEN: [f32; 3] = [0.0, 0.5, 0.0];
const RED: [f32; 3] = [0.75, 0.0, 0.0];
const YELLOW: [f32; 3] = [0.75, 0.75, 0.0];

struct Published {
    canvas: CanvasPublication,
    gui: GuiCanvasPublication,
}

impl Published {
    fn read(host: &HostRuntime, root: OutputRef) -> Self {
        let publication = host
            .publication(host.latest_publication(root.world().id()).unwrap())
            .unwrap();
        let canvas = publication
            .output(root)
            .unwrap()
            .data::<CanvasPublication>()
            .unwrap()
            .clone();
        let gui = publication
            .chunk(CanvasSystem::ID)
            .unwrap()
            .data::<GuiCanvasPublication>()
            .unwrap()
            .clone();

        assert_eq!(gui.views[&root].input_revision, canvas.input_revision);

        Self {
            canvas,
            gui,
        }
    }

    fn observation(&self, entity: EntityId) -> &GuiControlObservation {
        let view = &self.gui.views[&self.canvas.selection];
        let record = view
            .controls
            .iter()
            .find(|record| record.record.target.entity == entity)
            .unwrap();
        let observation = view.control(record.hit.target).unwrap();
        let hit = self
            .canvas
            .hits
            .iter()
            .find(|hit| hit.target == record.hit.target)
            .unwrap();

        assert_eq!(observation.hit, *hit);
        assert_eq!(
            observation.record.target.world,
            self.canvas.selection.world()
        );
        assert_eq!(observation.record.target.canvas_target(), hit.target);
        assert!(observation.available);
        assert!(observation.record.available);

        observation
    }

    fn part(&self, entity: EntityId, part: CanvasPart) -> Option<&Arc<CanvasPaintEntry>> {
        self.canvas.entries.iter().find(|entry| {
            let CanvasPaintEntry::Primitive {
                primitive,
                ..
            } = entry.as_ref()
            else {
                return false;
            };

            primitive.style().identity.target.entity == entity
                && primitive.style().identity.part == part
        })
    }

    fn shape(&self, entity: EntityId, part: CanvasPart, expected: [f32; 4]) {
        let CanvasPaintEntry::Primitive {
            primitive:
                CanvasPrimitive::Box {
                    style,
                    size,
                    ..
                },
            ..
        } = self.part(entity, part).unwrap().as_ref()
        else {
            panic!("control box");
        };

        assert_eq!(
            [style.position[0], style.position[1], size[0], size[1]],
            expected
        );
        assert_eq!(style.scale, [1.0; 2]);
    }

    fn glyphs(&self, entity: EntityId) -> &Arc<[CanvasGlyph]> {
        let CanvasPaintEntry::Primitive {
            primitive:
                CanvasPrimitive::Glyphs {
                    glyphs,
                    ..
                },
            ..
        } = self.part(entity, CanvasPart::Label).unwrap().as_ref()
        else {
            panic!("control label");
        };

        glyphs
    }
}

fn paint(part: GuiPartId, color: [f32; 3]) -> Result<GuiPaintPart> {
    Ok(GuiPaintPart {
        color: Some([color[0], color[1], color[2], 1.0]),
        ..GuiPaintPart::keyed(part)?
    })
}

/// A flat box of one colour: the row states away the default look's line, cut
/// corners, glow and check mark, which it would otherwise sit on.
fn plain(part: GuiPartId, color: [f32; 3]) -> Result<GuiPaintPart> {
    Ok(GuiPaintPart {
        border_width: Some(0.0),
        corner_cut: Some([0.0; 4]),
        glow_intensity: Some(0.0),
        shape: Some(0.0),
        ..paint(part, color)?
    })
}

fn parts(values: impl IntoIterator<Item = GuiPaintPart>) -> Rows<GuiPaintPart> {
    let mut rows = Rows::default();

    for value in values {
        rows.push(value).unwrap();
    }

    rows
}

/// A control under the panel entity that carries the fixture font.
fn control(
    host: &mut HostRuntime,
    root: OutputRef,
    panel: EntityId,
    value: ComponentValue,
    rect: [f32; 4],
    skin: GuiSkin,
) -> Result<EntityId> {
    let padding = if matches!(
        &value,
        ComponentValue::GuiButton(_) | ComponentValue::GuiTextInput(_)
    ) {
        4.0
    } else {
        0.0
    };

    let entity = place(
        host,
        root,
        vec![
            value,
            ComponentValue::GuiLayout(GuiLayout {
                width: rect[2],
                height: rect[3],
                padding_left: padding,
                padding_top: padding,
                clip: true,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: rect[0],
                y: rect[1],
                ..Default::default()
            }),
            ComponentValue::GuiSkin(skin),
        ],
    )?;
    apply(
        host,
        root.world().id(),
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(entity),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(panel)),
                before: None,
            },
        }],
    )?;
    Ok(entity)
}

/// The observed control's component, read from the World's component store.
fn stored(host: &mut HostRuntime, observation: &GuiControlObservation) -> ComponentValue {
    let target = observation.record.target;

    host.world_mut(target.world.id())
        .unwrap()
        .inspect(target.entity)
        .unwrap()
        .components
        .into_iter()
        .find(|value| value.type_id() == target.component)
        .unwrap()
}

fn stored_checked(host: &mut HostRuntime, observation: &GuiControlObservation) -> bool {
    let ComponentValue::GuiCheckbox(checkbox) = stored(host, observation) else {
        panic!("checkbox");
    };

    checkbox.checked
}

fn stored_scalar(host: &mut HostRuntime, observation: &GuiControlObservation) -> f32 {
    let ComponentValue::GuiSlider(slider) = stored(host, observation) else {
        panic!("slider");
    };

    assert_eq!(
        observation.record.value,
        GuiRoutingValue::Scalar(slider.value)
    );

    slider.value
}

fn stored_text(host: &mut HostRuntime, observation: &GuiControlObservation) -> Arc<str> {
    let ComponentValue::GuiTextInput(input) = stored(host, observation) else {
        panic!("text input");
    };

    input.text
}

/// Replace an observed control value with compare-and-set on its field.
fn replace(
    host: &mut HostRuntime,
    observation: &GuiControlObservation,
    offset: usize,
    expected: FieldValue,
    value: FieldValue,
) -> Result<()> {
    let target = observation.record.target;

    apply(
        host,
        target.world.id(),
        vec![Command::set_field_if(
            EntityRef::Handle(target.entity),
            target.component,
            FieldWrite {
                offset: offset as u32,
                value,
            },
            expected,
        )],
    )?;

    Ok(())
}

fn count_ink(pixels: &[u8], bounds: [usize; 4], yellow: bool) -> usize {
    (bounds[1]..bounds[3])
        .flat_map(|row| (bounds[0]..bounds[2]).map(move |column| (row * 256 + column) * 4))
        .filter(|offset| {
            let pixel = &pixels[*offset..*offset + 3];
            pixel[0] > 160
                && pixel[1] > 160
                && if yellow {
                    pixel[2] < pixel[1] / 2
                } else {
                    pixel[2] > 160
                }
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
    let world = host.create_world(Default::default(), &super::selection::panel())?;
    let root = canvas(&mut host, world, 100.0)?;

    place(
        &mut host,
        root,
        vec![
            ComponentValue::CanvasBox(CanvasBox {
                width: 256.0,
                height: 256.0,
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

    let font = AssetSource {
        kind: FONT_TYPE,
        uri: format!("producer://{}/17/200", world.0).into(),
        variant: 0,
    };
    let bytes = std::fs::read(fonts.join("shure-tech-mono.ippf"))?;
    let progress_limit = bytes
        .len()
        .div_ceil(ipp_core::services::asset_management::STREAM_CAPACITY)
        + 32;
    host.asset_resources_mut()
        .register_client_source(world, font.clone(), bytes)?;
    let font_key = host.asset_resources().find(&font).unwrap();
    let panel = place(
        &mut host,
        root,
        vec![ComponentValue::GuiFont(GuiFont {
            source: font.uri,
            variant: 0,
            font_size: 16.0,
        })],
    )?;

    let checkbox_parts = parts([
        plain(GuiPartId::base(GuiPrimitivePart::Background), GREY)?,
        plain(GuiPartId::base(GuiPrimitivePart::Icon), GREEN)?,
    ]);

    let theme = create(
        &mut host,
        world,
        vec![ComponentValue::GuiTheme(GuiTheme {
            parts: checkbox_parts.clone(),
            ..Default::default()
        })],
    )?;

    let mut checkbox_ids = Vec::new();
    for (checked, rect) in [
        (true, [16.0, 16.0, 32.0, 32.0]),
        (false, [64.0, 16.0, 32.0, 32.0]),
        (true, [112.0, 8.0, 20.0, 64.0]),
        (false, [148.0, 8.0, 20.0, 64.0]),
    ] {
        checkbox_ids.push(control(
            &mut host,
            root,
            panel,
            ComponentValue::GuiCheckbox(GuiCheckbox {
                checked,
                ..Default::default()
            }),
            rect,
            GuiSkin {
                theme,
                ..Default::default()
            },
        )?);
    }

    let mut explicit_parts = checkbox_parts;
    explicit_parts
        .push(paint(
            GuiPartId::variant(
                GuiPrimitivePart::Icon,
                GuiSkinState::Idle,
                GuiPartVariant::Unchecked,
            ),
            RED,
        )?)
        .unwrap();
    let explicit_theme = create(
        &mut host,
        world,
        vec![ComponentValue::GuiTheme(GuiTheme {
            parts: explicit_parts,
            ..Default::default()
        })],
    )?;
    let explicit = control(
        &mut host,
        root,
        panel,
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
        [188.0, 16.0, 32.0, 32.0],
        GuiSkin {
            theme: explicit_theme,
            ..Default::default()
        },
    )?;

    let slider = control(
        &mut host,
        root,
        panel,
        ComponentValue::GuiSlider(GuiSlider {
            value: 0.0,
            min: -2.0,
            max: 6.0,
            step: 0.5,
            ..Default::default()
        }),
        [16.0, 88.0, 144.0, 24.0],
        GuiSkin {
            parts: parts([
                plain(GuiPartId::base(GuiPrimitivePart::Background), GREY)?,
                plain(GuiPartId::base(GuiPrimitivePart::Fill), [0.0, 0.0, 0.75])?,
                plain(GuiPartId::base(GuiPrimitivePart::Icon), YELLOW)?,
            ]),
            ..Default::default()
        },
    )?;

    let button_theme = create(
        &mut host,
        world,
        vec![ComponentValue::GuiTheme(GuiTheme {
            parts: parts([
                plain(
                    GuiPartId::base(GuiPrimitivePart::Background),
                    [0.0, 0.0, 0.5],
                )?,
                paint(
                    GuiPartId::state(GuiPrimitivePart::Background, GuiSkinState::Disabled),
                    [0.5, 0.0, 0.0],
                )?,
                paint(GuiPartId::base(GuiPrimitivePart::Label), [1.0, 1.0, 0.0])?,
                paint(
                    GuiPartId::base(GuiPrimitivePart::FocusRing),
                    [0.75, 0.0, 0.75],
                )?,
            ]),
            ..Default::default()
        })],
    )?;

    let button = control(
        &mut host,
        root,
        panel,
        ComponentValue::GuiButton(GuiButton {
            label: "GO".into(),
            ..Default::default()
        }),
        [16.0, 144.0, 96.0, 32.0],
        GuiSkin {
            theme: button_theme,
            ..Default::default()
        },
    )?;

    let disabled = control(
        &mut host,
        root,
        panel,
        ComponentValue::GuiButton(GuiButton {
            label: "NO".into(),
            ..Default::default()
        }),
        [16.0, 200.0, 96.0, 24.0],
        GuiSkin {
            theme: button_theme,
            ..Default::default()
        },
    )?;
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(disabled),
            ComponentValue::GuiBehavior(GuiBehavior {
                enabled: false,
                ..Default::default()
            }),
        )],
    )?;

    let text = control(
        &mut host,
        root,
        panel,
        ComponentValue::GuiTextInput(GuiTextInput {
            text: "OO".into(),
            placeholder: "idle".into(),
            ..Default::default()
        }),
        [128.0, 144.0, 112.0, 32.0],
        GuiSkin {
            parts: parts([
                plain(
                    GuiPartId::base(GuiPrimitivePart::Background),
                    [0.0, 0.125, 0.0],
                )?,
                paint(GuiPartId::base(GuiPrimitivePart::Label), [0.75; 3])?,
            ]),
            ..Default::default()
        },
    )?;

    let viewport = WorldViewport {
        width: 256,
        height: 256,
        device_pixel_ratio: 1.0,
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

    let initial = Published::read(&host, root);
    assert_eq!(initial.gui.views[&root].controls.len(), 9);
    assert_eq!(initial.canvas.logical_extent, [256.0; 2]);
    for (entity, bounds, kind) in [
        (
            checkbox_ids[0],
            [16.0, 16.0, 48.0, 48.0],
            GuiControlKind::Checkbox,
        ),
        (
            checkbox_ids[1],
            [64.0, 16.0, 96.0, 48.0],
            GuiControlKind::Checkbox,
        ),
        (
            checkbox_ids[2],
            [112.0, 8.0, 132.0, 72.0],
            GuiControlKind::Checkbox,
        ),
        (
            checkbox_ids[3],
            [148.0, 8.0, 168.0, 72.0],
            GuiControlKind::Checkbox,
        ),
        (
            explicit,
            [188.0, 16.0, 220.0, 48.0],
            GuiControlKind::Checkbox,
        ),
        (slider, [16.0, 88.0, 160.0, 112.0], GuiControlKind::Slider),
        (button, [16.0, 144.0, 112.0, 176.0], GuiControlKind::Button),
        (
            text,
            [128.0, 144.0, 240.0, 176.0],
            GuiControlKind::TextInput,
        ),
    ] {
        let observation = initial.observation(entity);
        assert_eq!(observation.record.kind, kind);

        assert_eq!(observation.hit.bounds, bounds);

        assert_eq!(observation.hit.clip, bounds);

        assert_eq!(observation.hit.position, [bounds[0], bounds[1]]);
        assert!(observation.hit.contains([bounds[0], bounds[1]]));
        assert!(!observation.hit.contains([bounds[2], bounds[1]]));
        assert!(!observation.hit.contains([bounds[0], bounds[3]]));
    }

    assert!(!initial.observation(disabled).record.enabled);

    assert!(!initial.observation(disabled).hit.eligible);

    assert!(!initial.observation(disabled).hit.contains([20.0, 210.0]));

    assert!(
        host.world_mut(world)
            .unwrap()
            .gui_focus_page(0, 0, 1)
            .is_empty()
    );
    for (index, entity) in checkbox_ids.iter().enumerate() {
        assert_eq!(
            stored_checked(&mut host, initial.observation(*entity)),
            index % 2 == 0
        );
    }
    assert!(!stored_checked(&mut host, initial.observation(explicit)));
    assert_eq!(stored_scalar(&mut host, initial.observation(slider)), 0.0);
    assert_eq!(&*stored_text(&mut host, initial.observation(text)), "OO");

    for entity in &checkbox_ids {
        assert!(initial.part(*entity, CanvasPart::FocusRing).is_none());
    }
    assert!(initial.part(button, CanvasPart::FocusRing).is_none());
    assert!(initial.part(text, CanvasPart::FocusRing).is_none());

    initial.shape(checkbox_ids[0], CanvasPart::Icon, [24.0, 24.0, 16.0, 16.0]);
    initial.shape(checkbox_ids[2], CanvasPart::Icon, [117.0, 35.0, 10.0, 10.0]);
    initial.shape(explicit, CanvasPart::Icon, [196.0, 24.0, 16.0, 16.0]);
    assert!(initial.part(checkbox_ids[1], CanvasPart::Icon).is_none());
    assert!(initial.part(checkbox_ids[3], CanvasPart::Icon).is_none());

    let rail = initial.observation(slider).slider.unwrap();
    assert_eq!([rail.min, rail.max, rail.step], [-2.0, 6.0, 0.5]);
    assert_eq!(rail.thumb_centers, [9.0, 135.0]);
    assert_eq!(rail.thumb_rect, [31.5, 3.0, 18.0, 18.0]);
    // The rail is one scroll bar thick, half the panel's 16-unit font, centred.
    initial.shape(slider, CanvasPart::Background, [16.0, 96.0, 144.0, 8.0]);
    initial.shape(slider, CanvasPart::Fill, [16.0, 96.0, 40.5, 8.0]);
    initial.shape(slider, CanvasPart::Icon, [47.5, 91.0, 18.0, 18.0]);
    assert_eq!(initial.glyphs(button).len(), 2);
    assert_eq!(initial.glyphs(text).len(), 2);

    let first = capture()?;
    save(output, "gui-controls-initial", &first)?;
    for (point, color) in [
        ([32, 32], GREEN),
        ([20, 20], GREY),
        ([14, 32], [0.0; 3]),
        ([80, 32], GREY),
        ([122, 40], GREEN),
        ([114, 40], GREY),
        ([129, 40], GREY),
        ([122, 20], GREY),
        ([122, 60], GREY),
        ([158, 40], GREY),
        ([204, 32], RED),
        ([56, 94], YELLOW),
        ([24, 100], [0.0, 0.0, 0.75]),
        ([140, 100], GREY),
        ([80, 92], [0.0; 3]),
        ([165, 100], [0.0; 3]),
        ([108, 172], [0.0, 0.0, 0.5]),
        ([14, 160], [0.0; 3]),
        ([108, 220], [0.5, 0.0, 0.0]),
        ([232, 168], [0.0, 0.125, 0.0]),
    ] {
        assert_color(&first, point[0], point[1], color);
    }

    let button_ink = count_ink(&first, [16, 144, 112, 176], true);
    let first_ink = count_ink(&first, [128, 144, 240, 176], false);
    assert!(button_ink > 25, "button label contours: {button_ink}");
    assert!(first_ink > 25, "text input contours: {first_ink}");

    frame_at(renderer, &mut host, root, viewport, 0.0)?;
    let warm = Published::read(&host, root);
    assert!(Arc::ptr_eq(&initial.gui.views, &warm.gui.views));
    assert!(Arc::ptr_eq(&initial.canvas.entries, &warm.canvas.entries));
    assert_eq!(renderer.statistics().gui_rebuilds, 0);
    assert_eq!(renderer.statistics().uploaded_bytes, 0);
    assert_eq!(capture()?, first);
    let work = host
        .world_mut(world)
        .unwrap()
        .gui_entity_layout_statistics()
        .unwrap()
        .total;

    for (index, entity) in checkbox_ids.iter().enumerate() {
        replace(
            &mut host,
            initial.observation(*entity),
            offset_of!(GuiCheckbox, checked),
            FieldValue::Bool(index % 2 == 0),
            FieldValue::Bool(index % 2 == 1),
        )?;
    }
    replace(
        &mut host,
        initial.observation(slider),
        offset_of!(GuiSlider, value),
        FieldValue::F32(0.0),
        FieldValue::F32(4.0),
    )?;
    frame_at(renderer, &mut host, root, viewport, 0.1)?;

    let changed = Published::read(&host, root);
    assert_ne!(changed.canvas.input_revision, initial.canvas.input_revision);

    for (index, entity) in checkbox_ids.iter().enumerate() {
        let before = initial.observation(*entity);
        let after = changed.observation(*entity);
        assert_eq!(after.record.target, before.record.target);
        assert_eq!(stored_checked(&mut host, after), index % 2 == 1);
        assert_eq!(after.hit.bounds, before.hit.bounds);
    }

    assert!(changed.part(checkbox_ids[0], CanvasPart::Icon).is_none());
    assert!(changed.part(checkbox_ids[2], CanvasPart::Icon).is_none());
    changed.shape(checkbox_ids[1], CanvasPart::Icon, [72.0, 24.0, 16.0, 16.0]);
    changed.shape(checkbox_ids[3], CanvasPart::Icon, [153.0, 35.0, 10.0, 10.0]);

    assert_eq!(
        changed.observation(slider).record.target,
        initial.observation(slider).record.target
    );
    assert_eq!(stored_scalar(&mut host, changed.observation(slider)), 4.0);
    assert_eq!(
        changed.observation(slider).slider.unwrap().thumb_rect,
        [94.5, 3.0, 18.0, 18.0]
    );
    changed.shape(slider, CanvasPart::Fill, [16.0, 96.0, 103.5, 8.0]);
    changed.shape(slider, CanvasPart::Icon, [110.5, 91.0, 18.0, 18.0]);

    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .gui_entity_layout_statistics()
            .unwrap()
            .total,
        work
    );
    assert!(Arc::ptr_eq(
        initial.part(button, CanvasPart::Background).unwrap(),
        changed.part(button, CanvasPart::Background).unwrap()
    ));
    assert!(Arc::ptr_eq(initial.glyphs(button), changed.glyphs(button)));
    assert!(Arc::ptr_eq(initial.glyphs(text), changed.glyphs(text)));

    let changed_pixels = capture()?;
    save(output, "gui-controls-values", &changed_pixels)?;
    for (point, color) in [
        ([32, 32], GREY),
        ([80, 32], GREEN),
        ([122, 40], GREY),
        ([158, 40], GREEN),
        ([119, 94], YELLOW),
        ([56, 92], [0.0; 3]),
        ([96, 100], [0.0, 0.0, 0.75]),
        ([140, 100], GREY),
        ([204, 32], RED),
    ] {
        assert_color(&changed_pixels, point[0], point[1], color);
    }

    replace(
        &mut host,
        changed.observation(text),
        offset_of!(GuiTextInput, text),
        FieldValue::String("OO".into()),
        FieldValue::String("OOOO".into()),
    )?;

    frame_at(renderer, &mut host, root, viewport, 0.2)?;

    let edited = Published::read(&host, root);
    assert_eq!(edited.glyphs(text).len(), 4);
    assert!(!Arc::ptr_eq(initial.glyphs(text), edited.glyphs(text)));
    assert!(Arc::ptr_eq(initial.glyphs(button), edited.glyphs(button)));
    assert_eq!(
        edited.observation(text).record.target,
        initial.observation(text).record.target
    );
    assert_eq!(&*stored_text(&mut host, edited.observation(text)), "OOOO");
    assert_eq!(edited.observation(text).hit, changed.observation(text).hit);

    let edited_pixels = capture()?;
    save(output, "gui-controls-text", &edited_pixels)?;
    let edited_ink = count_ink(&edited_pixels, [128, 144, 240, 176], false);
    assert!(
        edited_ink > first_ink * 3 / 2,
        "text edit contours: {first_ink} -> {edited_ink}"
    );
    assert_color(&edited_pixels, 119, 94, YELLOW);

    frame_at(renderer, &mut host, root, viewport, 0.2)?;
    let final_warm = Published::read(&host, root);
    assert!(Arc::ptr_eq(&edited.gui.views, &final_warm.gui.views));
    assert!(Arc::ptr_eq(
        &edited.canvas.entries,
        &final_warm.canvas.entries
    ));
    assert_eq!(renderer.statistics().gui_rebuilds, 0);
    assert_eq!(renderer.statistics().uploaded_bytes, 0);
    assert_eq!(capture()?, edited_pixels);

    std::fs::write(
        output.join("gui-controls.txt"),
        format!(
            "viewport={viewport:?}\ncontrols=9\ncheckbox_initial=true,false,true,false\ncheckbox_changed=false,true,false,true\nexplicit_unchecked=true\nslider_rail={rail:?}\nslider_changed={:?}\nbutton_ink={button_ink}\ntext_ink={first_ink}->{edited_ink}\ninitial_revision={}\nchanged_revision={}\nfinal_revision={}\nfocus_scope=unfocused-only; focus/cache coverage in gui-cache-interaction\n",
            changed.observation(slider).slider,
            initial.canvas.input_revision,
            changed.canvas.input_revision,
            edited.canvas.input_revision,
        ),
    )?;

    renderer.prepare(&mut host, None)?;
    renderer.unload_host(&mut host)?;

    Ok(())
}
