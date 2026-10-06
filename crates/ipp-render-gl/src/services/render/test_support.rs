//! Unit-test fixtures shared by renderer tests.

use super::outputs::scene::RenderEntity;
use ipp_core::{EntityId, HostRuntime, WorldRef};

pub(super) fn test_entity(bits: u64) -> RenderEntity {
    static WORLD: std::sync::OnceLock<WorldRef> = std::sync::OnceLock::new();
    let world = *WORLD.get_or_init(|| {
        let mut host = HostRuntime::new();
        let world = host.create_world(Default::default(), &[]).unwrap();
        host.world_ref(world).unwrap()
    });
    RenderEntity {
        world,
        entity: EntityId::from_bits(bits),
        incarnation: 1,
    }
}
