use super::GuiInputError;
use super::keyboard_panels::GuiKeyboardView;
use super::routing::{Target, live};
use crate::services::gui_input::query::GuiQueryWorlds;
use crate::systems::camera::CameraPublication;
use crate::systems::canvas::{CanvasHitKind, CanvasPublication, CanvasSystem};
use crate::systems::geometry::GeometryShapeTransform;
use crate::systems::gui::local::GuiControlKind;
use crate::systems::gui::presentation::GuiCanvasPublication;
use crate::{
    HostRuntime, OutputKind, OutputRef, ViewDescriptor, WorldAttachmentMode, WorldAttachmentToken,
    WorldPublicationId,
};

struct View {
    publication: WorldPublicationId,
    output: OutputRef,
    path: Vec<WorldAttachmentToken>,
}

enum Task {
    View(View),
    Control(Target),
}

/// Enter one World of the traversal. Traversal inherently visits every
/// eligible control; entering each World once bounds it by the composition.
fn enter(worlds: &mut GuiQueryWorlds, world: crate::WorldRef) -> Result<(), GuiInputError> {
    worlds.enter(world).map_err(|_| GuiInputError::Unavailable)
}

pub(super) fn targets(
    host: &HostRuntime,
    root: ViewDescriptor,
    focused: Option<&Target>,
) -> Result<Vec<Target>, GuiInputError> {
    let mut worlds = GuiQueryWorlds::default();
    let mut pending = vec![Task::View(View {
        publication: root.publication,
        output: root.output,
        path: Vec::new(),
    })];
    let mut controls = Vec::new();
    while let Some(task) = pending.pop() {
        let view = match task {
            Task::Control(control) => {
                controls.push(control);
                continue;
            }
            Task::View(view) => view,
        };
        enter(&mut worlds, view.output.world())?;
        let publication = host
            .publication(view.publication)
            .ok_or(GuiInputError::StalePath)?;
        if live(host, publication.world).is_err() {
            continue;
        }
        match view.output.kind() {
            OutputKind::Camera => pending.extend(
                camera(host, view, &mut worlds)?
                    .into_iter()
                    .rev()
                    .map(Task::View),
            ),
            OutputKind::Canvas => {
                let Some(canvas) = host
                    .output(view.publication, view.output)
                    .and_then(|chunk| chunk.data::<CanvasPublication>())
                else {
                    continue;
                };
                let gui = publication
                    .chunk(CanvasSystem::ID)
                    .and_then(|chunk| chunk.data::<GuiCanvasPublication>())
                    .and_then(|gui| gui.views.get(&view.output));
                let mut items = Vec::new();
                if let Some(gui) = gui {
                    for control in gui.controls.iter() {
                        if control.available
                            && control.hit.eligible
                            && !matches!(
                                control.record.kind,
                                GuiControlKind::ScrollView | GuiControlKind::VirtualList
                            )
                        {
                            items.push((
                                control.hit.paint_order,
                                Task::Control(Target {
                                    control: control.clone(),
                                    path: view.path.clone(),
                                    source: root.publication,
                                    part: CanvasHitKind::Entity,
                                }),
                            ));
                        }
                    }
                }
                for hit in canvas.hits.iter() {
                    let CanvasHitKind::Attachment {
                        token,
                        ..
                    } = &hit.kind
                    else {
                        continue;
                    };
                    if !hit.eligible {
                        continue;
                    }
                    let Some(edge) = publication
                        .attachments
                        .iter()
                        .find(|edge| edge.token == *token)
                    else {
                        continue;
                    };
                    let (Some(child), Some(output)) =
                        (host.attached_publication(edge), edge.output)
                    else {
                        continue;
                    };
                    let mut path = view.path.clone();
                    path.push(token.clone());
                    items.push((
                        hit.paint_order,
                        Task::View(View {
                            publication: child.id,
                            output,
                            path,
                        }),
                    ));
                }
                items.sort_by_key(|(order, _)| *order);
                pending.extend(items.into_iter().rev().map(|(_, item)| item));
            }
        }
    }
    if let Some(focus) = focused
        && let Some(scope) = focus.control.focus_scope
    {
        controls.retain(|candidate| {
            candidate.control.record.target.world == focus.control.record.target.world
                && candidate.path == focus.path
                && candidate.control.record.ancestry.contains(&scope)
        });
    }
    Ok(controls)
}

fn camera(
    host: &HostRuntime,
    view: View,
    worlds: &mut GuiQueryWorlds,
) -> Result<Vec<View>, GuiInputError> {
    let camera = host
        .output(view.publication, view.output)
        .and_then(|chunk| chunk.data::<CameraPublication>())
        .and_then(GuiKeyboardView::of_publication)
        .ok_or(GuiInputError::Unavailable)?;
    let mut spatial = vec![(
        view.publication,
        GeometryShapeTransform::default(),
        view.path,
    )];
    let mut panels = Vec::new();
    while let Some((source, parent, path)) = spatial.pop() {
        let publication = host.publication(source).ok_or(GuiInputError::StalePath)?;
        if live(host, publication.world).is_err() {
            continue;
        }
        for edge in &publication.attachments {
            if edge
                .placement_output
                .is_some_and(|owner| owner != view.output)
            {
                continue;
            }
            let Some(child) = host.attached_publication(edge) else {
                continue;
            };
            let affine = GeometryShapeTransform::new(edge.placement)
                .and_then(|placement| placement.then(&parent))
                .map_err(|_| GuiInputError::Unavailable)?;
            let mut next_path = path.clone();
            next_path.push(edge.token.clone());
            if edge.mode == WorldAttachmentMode::Spatial {
                enter(worlds, child.world)?;
                spatial.push((child.id, affine, next_path));
            } else if let (Some(extent), Some(output)) = (edge.surface_extent, edge.output)
                && let Some((front, distance)) = camera.placement(&affine, extent)
            {
                panels.push((
                    !front,
                    distance,
                    publication.world,
                    edge.anchor,
                    View {
                        publication: child.id,
                        output,
                        path: next_path,
                    },
                ));
            }
        }
    }
    panels.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.total_cmp(&right.1))
            .then(left.2.cmp(&right.2))
            .then(left.3.cmp(&right.3))
    });
    Ok(panels.into_iter().map(|(_, _, _, _, view)| view).collect())
}
