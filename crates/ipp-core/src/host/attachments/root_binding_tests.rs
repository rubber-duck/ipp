use super::*;
use crate::host::test_support::camera;

#[test]
fn explicit_root_rebind_is_host_fenced_never_aba_and_publication_refresh_is_not_rebind() {
    let mut host = HostRuntime::new();
    let output = camera(&mut host);
    let viewport = WorldViewport {
        width: 640,
        height: 480,
        device_pixel_ratio: 2.0,
    };
    assert_eq!(host.root_output_binding(output.world()), Ok(None));
    host.set_root_output(output, viewport).unwrap();
    let first = host.root_output_binding(output.world()).unwrap().unwrap();
    host.frame(0.0).unwrap();
    host.frame(0.1).unwrap();
    assert_eq!(host.root_output_binding(output.world()), Ok(Some(first)));
    host.set_root_output(output, viewport).unwrap();
    let second = host.root_output_binding(output.world()).unwrap().unwrap();
    assert_ne!(first.generation, second.generation);
    assert_eq!(first.generation.identity(), (host.topology.identity, 1));
    assert_eq!(second.generation.identity(), (host.topology.identity, 2));
    assert_eq!(first.output, second.output);
    assert_eq!(first.viewport, second.viewport);
    host.clear_root_output(output.world().id());
    assert_eq!(host.root_output_binding(output.world()), Ok(None));
    host.set_root_output(
        output,
        WorldViewport {
            width: 800,
            ..viewport
        },
    )
    .unwrap();
    let third = host.root_output_binding(output.world()).unwrap().unwrap();
    assert_ne!(second.generation, third.generation);
    assert_eq!(third.viewport.width, 800);
    assert_eq!(
        host.set_root_output(
            output,
            WorldViewport {
                width: 0,
                ..viewport
            }
        ),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(host.root_output_binding(output.world()), Ok(Some(third)));
    host.topology.next_root_binding = u64::MAX;
    assert_eq!(
        host.set_root_output(output, viewport),
        Err(ErrorReason::Capacity)
    );
    assert_eq!(host.root_output_binding(output.world()), Ok(Some(third)));
    assert!(host.destroy_world(output.world().id()));
    assert_eq!(
        host.root_output_binding(output.world()),
        Err(ErrorReason::InvalidEntity)
    );
    let replacement = camera(&mut host);
    assert_eq!(host.root_output_binding(replacement.world()), Ok(None));
    assert_eq!(
        host.set_root_output(replacement, viewport),
        Err(ErrorReason::Capacity)
    );

    let mut other = HostRuntime::new();
    let other_output = camera(&mut other);
    other.set_root_output(other_output, viewport).unwrap();
    let other_root = other
        .root_output_binding(other_output.world())
        .unwrap()
        .unwrap();
    assert_ne!(first.generation, other_root.generation);
    assert_ne!(
        first.generation.identity().0,
        other_root.generation.identity().0
    );
    assert_eq!(
        first.generation.identity().1,
        other_root.generation.identity().1
    );
    assert_eq!(
        host.root_output_binding(other_output.world()),
        Err(ErrorReason::InvalidEntity)
    );
}
