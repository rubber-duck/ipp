use super::*;
use ipp_core::{Batch, Command, ComponentValue, EntityRef};

/// Queue one laid-out canvas; the next recorded Host frame applies and evaluates it.
fn canvas(host: &mut HostRuntime) -> WorldRef {
    let identity = host
        .create_world(
            Default::default(),
            &[
                ipp_core::systems::canvas::CanvasSystem::ID,
                ipp_core::systems::gui::GuiSystem::ID,
                ipp_core::systems::gui::GuiLayoutSystem::ID,
            ],
        )
        .unwrap();
    host.world_mut(identity)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(1),
                    ComponentValue::GuiLayout(ipp_core::systems::gui::layout::GuiLayout {
                        width: 20.0,
                        height: 10.0,
                        ..Default::default()
                    }),
                ),
            ],
        })
        .unwrap();

    host.world_ref(identity).unwrap()
}

fn record(host: &mut HostRuntime, statistics: &mut HostGuiLayoutStatistics) {
    let frame = host.frame(0.01).unwrap();
    statistics.record(host, &frame);
    let words = statistics.words();
    statistics.record(host, &frame);
    assert_eq!(statistics.words(), words);
}

#[test]
fn gui_layout_statistics_include_unpresented_worlds_once_and_retain_retired_totals() {
    let mut host = HostRuntime::new();
    let first = canvas(&mut host);
    let second = canvas(&mut host);
    let mut statistics = HostGuiLayoutStatistics::default();
    assert_eq!(statistics.words(), None);
    record(&mut host, &mut statistics);
    assert_eq!(statistics.worlds.len(), 2);
    assert_eq!(statistics.words().unwrap()[0], 2);
    assert_eq!(statistics.words().unwrap()[2], 2);
    record(&mut host, &mut statistics);
    assert_eq!(statistics.words().unwrap()[0], 0);
    assert_eq!(statistics.words().unwrap()[2], 2);
    host.destroy_world(first.id());
    let replacement = canvas(&mut host);
    assert_ne!(first, replacement);
    record(&mut host, &mut statistics);
    assert_eq!(statistics.worlds.len(), 2);
    assert_eq!(statistics.retired_worlds, 1);
    assert_eq!(statistics.words().unwrap()[2], 3);
    assert!(
        statistics
            .worlds
            .iter()
            .any(|sample| sample.world == second)
    );
    assert!(!statistics.worlds.iter().any(|sample| sample.world == first));
}

#[test]
fn gui_layout_statistics_do_not_hide_counter_resets() {
    let mut host = HostRuntime::new();
    let world = canvas(&mut host);
    let mut statistics = HostGuiLayoutStatistics::default();
    record(&mut host, &mut statistics);
    assert_eq!(statistics.worlds[0].status, "evaluated");
    assert_eq!(statistics.words().unwrap()[2], 1);
    statistics.previous.get_mut(&world).unwrap().reflows = 100;
    record(&mut host, &mut statistics);
    assert_eq!(statistics.words(), None);
    let mut json = String::new();
    statistics.write_json(&mut json);
    assert!(json.contains("\"complete\":false"));
    assert!(json.contains("\"latest\":null,\"total\":null"));
}

#[test]
fn gui_layout_statistics_preserve_unselected_layout_as_missing() {
    let mut host = HostRuntime::new();
    host.create_world(Default::default(), &[]).unwrap();
    let mut statistics = HostGuiLayoutStatistics::default();
    record(&mut host, &mut statistics);
    assert_eq!(statistics.words(), None);
    assert_eq!(statistics.worlds.len(), 1);
    assert!(statistics.worlds[0].statistics.is_none());
    let mut json = String::new();
    statistics.write_json(&mut json);
    assert!(json.contains("\"layout\":null"));
}
