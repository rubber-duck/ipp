//! Real GLES projection under device-clamped resolution, joined to completed composed queries.

use super::canvas_publications::{canvas, frame_at, place};
use super::publications::{assert_color, camera, create, mesh, save};
use ipp_core::components::rows::Rows;
use ipp_core::components::{CanvasStyle, GuiButton, GuiLayout, Surface, Transform};
use ipp_core::services::gui_input::query::{
    GuiQueryOutcome, project_composed_point, query_composed_input,
};
use ipp_core::systems::gui::GuiPartId;
use ipp_core::systems::gui::GuiPrimitivePart;
use ipp_core::systems::gui::presentation::{GuiPaintPart, GuiSkin};
use ipp_core::{ComponentValue, HostRuntime, ViewQueryTarget, WorldAttachment, WorldViewport};
use ipp_render_gl::{RenderDevice, RenderService};
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    capture: impl Fn() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let parent = host.create_world(Default::default(), &super::selection::panel())?;
    let camera_world = host.create_world(Default::default(), &super::selection::scene())?;
    let panel_world = host.create_world(Default::default(), &super::selection::panel())?;
    let root = canvas(&mut host, parent, 1.0)?;
    let nested = camera(&mut host, camera_world, 2.0)?;
    let panel = canvas(&mut host, panel_world, 100.0)?;
    let surface = Surface {
        width: 10_000_000.0,
        height: 1.0,
    };
    place(
        &mut host,
        root,
        vec![
            ComponentValue::Surface(surface),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 28.0,
                y: 78.0,
                scale_x: 0.00002,
                scale_y: 100.0,
                ..Default::default()
            }),
            ComponentValue::WorldAttachment(WorldAttachment::surface(nested)),
        ],
    )?;
    mesh(
        &mut host,
        camera_world,
        Transform {
            x: -3_000_000.0,
            sx: 2_000_000.0,
            sy: 2.0,
            ..Default::default()
        },
        [0.0, 0.5, 0.0],
    )?;
    let surface = Surface {
        width: 2_000_000.0,
        height: 2.0,
    };
    create(
        &mut host,
        camera_world,
        vec![
            ComponentValue::Transform(Transform {
                x: 3_000_000.0,
                ..Default::default()
            }),
            ComponentValue::Surface(surface),
            ComponentValue::WorldAttachment(WorldAttachment::surface(panel)),
        ],
    )?;
    let mut parts = Rows::default();
    parts
        .push(GuiPaintPart {
            color: Some([0.5, 0.0, 0.0, 1.0]),
            border_width: Some(0.0),
            corner_radius: Some([0.0; 2]),
            ..GuiPaintPart::keyed(GuiPartId::base(GuiPrimitivePart::Background))?
        })
        .unwrap();
    let control = place(
        &mut host,
        panel,
        vec![
            ComponentValue::GuiButton(GuiButton::default()),
            ComponentValue::GuiLayout(GuiLayout {
                width: 200_000_000.0,
                height: 200.0,
                align_x: -1.0,
                align_y: -1.0,
                ..Default::default()
            }),
            ComponentValue::GuiSkin(GuiSkin {
                parts,
                ..Default::default()
            }),
        ],
    )?;
    let viewport = WorldViewport {
        width: 256,
        height: 256,
        device_pixel_ratio: 1.0,
    };
    host.set_root_output(root, viewport)?;
    frame_at(renderer, &mut host, root, viewport, 0.0)?;
    let pixels = capture()?;
    save(output, "composed-query-physical-aspect", &pixels)?;
    assert_color(&pixels, 98, 128, [0.0, 0.5, 0.0]);
    assert_color(&pixels, 158, 128, [0.5, 0.0, 0.0]);
    let view = ViewQueryTarget::RootView {
        output: root,
        expected_viewport: viewport,
    };
    let point = [158.0 / 256.0, 0.5];
    let result = query_composed_input(&host, view, point, Default::default())?;
    let GuiQueryOutcome::Hit(hit) = result.outcome else {
        panic!("rendered red control must be queryable");
    };
    assert_eq!(hit.output, panel);
    assert_eq!(hit.hit.target.entity, control);
    assert_eq!(hit.path.len(), 2);
    assert!(hit.control.unwrap().available);
    assert!((hit.point[0] / 200_000_000.0 - 0.5).abs() < 1e-4);
    assert!((hit.point[1] - 100.0).abs() < 1e-4);
    let path: Vec<_> = hit.path.iter().map(|step| step.token.clone()).collect();
    let projected = project_composed_point(&host, view, &path, point, true)?.unwrap();
    assert_eq!(projected.output, panel);
    assert_eq!(projected.point, hit.point);
    let camera_path = &path[..1];
    let camera_view = host.resolve_camera_view(view, camera_path)?;
    assert_eq!(camera_view.extent, [10_000_000.0, 1.0]);
    let (_, position) = host.project_camera_view(
        view,
        camera_path,
        [0.65, 0.5],
        ipp_core::WorldPlane {
            point: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
        },
    )?;
    assert!((position.unwrap()[0] - 3_000_000.0).abs() < 1.0);
    let publication = host.root_output(parent).unwrap().2;
    let warm = renderer.draw(&host, root, publication, viewport, 0.0)?;
    assert_eq!(warm.failed_draw_calls, 0);
    assert_eq!(capture()?, pixels);
    renderer.prepare(&mut host, None)?;
    for world in [parent, camera_world, panel_world] {
        renderer.forget_world(world);
    }
    Ok(())
}
