use super::{create, update};
use ipp_core::components::{CanvasGlyphRun, CanvasStyle, Surface, Transform};
use ipp_core::systems::canvas::{CanvasGlyphRow, CanvasPublication};
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, HostRuntime, OutputRef, WorldAttachment,
    WorldId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasSurface {
    pub parent: WorldId,
    pub anchor: EntityId,
    pub output: OutputRef,
    pub content: EntityId,
}

impl CanvasSurface {
    pub fn new(
        host: &mut HostRuntime,
        parent: WorldId,
        z: f32,
        values: Vec<ComponentValue>,
    ) -> Self {
        Self::new_in(host, parent, z, values, &panel_systems())
    }

    /// [`CanvasSurface::new`] with an explicit panel World selection.
    pub fn new_in(
        host: &mut HostRuntime,
        parent: WorldId,
        z: f32,
        values: Vec<ComponentValue>,
        systems: &[ipp_core::systems::SystemId],
    ) -> Self {
        let child = host.create_world(Default::default(), systems).unwrap();
        let output = OutputRef::canvas(host.world_ref(child).unwrap());
        let content = add_content(host, output, values);
        let anchor = create(
            &mut host.world_mut(parent).unwrap(),
            vec![
                ComponentValue::Transform(Transform {
                    z,
                    ..Default::default()
                }),
                ComponentValue::Surface(Surface::default()),
                ComponentValue::WorldAttachment(WorldAttachment::surface(output)),
            ],
        );
        Self {
            parent,
            anchor,
            output,
            content,
        }
    }

    pub fn publication<'a>(&self, host: &'a HostRuntime) -> &'a CanvasPublication {
        let publication = host.latest_publication(self.output.world().id()).unwrap();
        host.output(publication, self.output)
            .unwrap()
            .data::<CanvasPublication>()
            .unwrap()
    }

    pub fn set_style(&self, host: &mut HostRuntime, style: CanvasStyle) {
        apply(
            host,
            self.output.world().id(),
            vec![Command::insert_value(
                EntityRef::Handle(self.content),
                ComponentValue::CanvasStyle(style),
            )],
        );
    }
}

pub fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) {
    let mut world = host.world_mut(world).unwrap();
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    let report = update(&mut world).unwrap();
    assert!(report.outcomes.iter().all(|outcome| outcome.result.is_ok()));
}

/// Top-level canvas content of the output's World.
pub fn add_content(
    host: &mut HostRuntime,
    output: OutputRef,
    values: Vec<ComponentValue>,
) -> EntityId {
    create(&mut host.world_mut(output.world().id()).unwrap(), values)
}

pub fn glyph_run(ids: &[u32], position: [f32; 2]) -> Vec<ComponentValue> {
    let mut glyphs = ipp_core::components::rows::Rows::new();
    for (index, &glyph_id) in ids.iter().enumerate() {
        glyphs
            .push(CanvasGlyphRow {
                glyph_id,
                position: [0.01 * index as f32, 0.0],
                color: None,
            })
            .unwrap();
    }
    vec![
        ComponentValue::CanvasGlyphRun(CanvasGlyphRun {
            source: "fixture:///font.ippf".into(),
            font_size: 1.0,
            glyphs,
            ..Default::default()
        }),
        ComponentValue::CanvasStyle(CanvasStyle {
            x: position[0],
            y: position[1],
            ..Default::default()
        }),
    ]
}

pub fn set_viewport(host: &mut HostRuntime, world: WorldId, width: u32, height: u32) {
    let (output, _, _) = host.root_output(world).unwrap();
    host.set_root_output(
        output,
        ipp_core::WorldViewport {
            width,
            height,
            device_pixel_ratio: 1.0,
        },
    )
    .unwrap();
}

/// A presented panel World: Canvas content with asset-backed text, Surface
/// slots for nested panels, controls and entity layout.
pub fn panel_systems() -> Vec<ipp_core::systems::SystemId> {
    use super::selection::{ATTACHMENTS, CANVAS_CONTENT, GUI_LAYOUT, SURFACE, select};
    select(&[ATTACHMENTS, CANVAS_CONTENT, GUI_LAYOUT, SURFACE])
}
