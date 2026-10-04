//! Diagnostic filtering and applied effects; real host coverage belongs to the harness.

mod support;

mod enabled {
    use crate::support::WorldTestDriver;

    use std::cell::{Cell, RefCell};
    use std::fmt;

    use ipp_core::diagnostics::{Level, configure};
    use ipp_core::{Batch, Command, EntityMetadata, EntityRef};

    thread_local! {
        static LINES: RefCell<Vec<(Level, String)>> = const { RefCell::new(Vec::new()) };
    }

    fn sink(level: Level, arguments: fmt::Arguments<'_>) {
        LINES.with_borrow_mut(|lines| lines.push((level, arguments.to_string())));
    }

    fn take() -> Vec<(Level, String)> {
        LINES.with_borrow_mut(std::mem::take)
    }

    fn create(alias: u32) -> Command {
        Command::Create {
            alias,
            metadata: EntityMetadata::default(),
            adopt: false,
        }
    }

    fn run(
        world: &mut ipp_core::WorldContext<'_>,
        id: u64,
        operations: Vec<Command>,
    ) -> ipp_core::WorldUpdateReport {
        world
            .enqueue(Batch {
                id,
                operations,
            })
            .unwrap();
        world.update_for_test(0.0).unwrap()
    }

    fn effects() -> Vec<String> {
        take()
            .into_iter()
            .map(|(_, line)| line)
            .filter(|line| line.contains("entity."))
            .collect()
    }

    #[test]
    fn levels_filter_before_evaluation_and_formatting() {
        let calls = Cell::new(0);
        struct Formatted<'a>(&'a Cell<u32>);
        impl fmt::Display for Formatted<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.set(self.0.get() + 1);
                f.write_str("formatted")
            }
        }

        configure(Level::Info, Some(sink));
        ipp_core::diagnostic!(Debug, "{}", {
            calls.set(10);
            Formatted(&calls)
        });
        assert_eq!(calls.get(), 0);
        ipp_core::diagnostic!(Info, "{}", Formatted(&calls));
        assert_eq!(calls.get(), 1);
        assert_eq!(take(), vec![(Level::Info, "formatted".into())]);

        for threshold in 0..=5 {
            configure(Level::from_u32(threshold).unwrap(), Some(sink));
            ipp_core::diagnostic!(Off, "off");
            ipp_core::diagnostic!(Error, "error");
            ipp_core::diagnostic!(Warn, "warn");
            ipp_core::diagnostic!(Info, "info");
            ipp_core::diagnostic!(Debug, "debug");
            ipp_core::diagnostic!(Trace, "trace");
            assert_eq!(take().len(), threshold as usize);
        }
        configure(Level::Trace, None);
        ipp_core::diagnostic!(Error, "{}", {
            calls.set(10);
            "never"
        });
        assert_eq!(calls.get(), 1);
        assert!(take().is_empty());
        assert!(Level::from_u32(6).is_none());
        for (index, name) in ["off", "error", "warn", "info", "debug", "trace"]
            .iter()
            .enumerate()
        {
            assert_eq!(Level::parse(name), Level::from_u32(index as u32));
        }
        for name in ["INFO", " info", "info ", "", "6"] {
            assert!(Level::parse(name).is_none());
        }
    }

    #[test]
    fn sinks_can_reconfigure_without_tls_borrows_or_recursive_logging() {
        fn reconfigure(level: Level, arguments: fmt::Arguments<'_>) {
            configure(Level::Trace, Some(sink));
            fn recursive_argument() -> u32 {
                panic!("recursive argument evaluated");
            }

            ipp_core::diagnostic!(Error, "{}", recursive_argument());
            sink(level, arguments);
        }

        configure(Level::Info, Some(reconfigure));
        ipp_core::diagnostic!(Info, "outer");
        ipp_core::diagnostic!(Debug, "after");
        assert_eq!(
            take(),
            vec![
                (Level::Info, "outer".into()),
                (Level::Debug, "after".into())
            ]
        );
        configure(Level::Off, None);
    }

    #[test]
    fn entity_effects_include_partial_failures_and_transient_recycled_handles() {
        configure(Level::Debug, Some(sink));
        let mut world_host = crate::support::task_scheduler::host();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default(), &[])
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let report = run(&mut world, 1, vec![create(1)]);
        let first = report.outcomes[0].result.as_ref().unwrap()[0].1;
        assert_eq!(
            effects(),
            vec![format!(
                "[IPP core] entity.create batch=1 entity={}",
                first.to_bits()
            )]
        );

        // Delete and create both apply successfully before a duplicate alias fails.
        let report = run(
            &mut world,
            2,
            vec![
                Command::Delete {
                    entity: EntityRef::Handle(first),
                },
                create(1),
                create(1),
            ],
        );
        assert!(report.outcomes[0].result.is_err());
        let lines = take();
        assert!(lines.iter().any(|(level, line)| *level == Level::Warn && line.contains("batch.reject batch=2")));
        let applied = report.outcomes[0].result.as_ref().unwrap_err().aliases[0].1;
        assert_eq!(
            lines
                .iter()
                .filter(|(_, line)| line.contains("entity."))
                .count(),
            2
        );
        assert!(world.inspect(first).is_none());
        assert!(world.inspect(applied).is_some());

        let report = run(
            &mut world,
            3,
            vec![
                Command::Delete {
                    entity: EntityRef::Handle(applied),
                },
                create(1),
                Command::Delete {
                    entity: EntityRef::Alias(1),
                },
            ],
        );
        let second = report.outcomes[0].result.as_ref().unwrap()[0].1;
        assert_ne!(first, second);
        assert_eq!(first.index(), second.index());
        assert_eq!(
            effects(),
            vec![
                format!(
                    "[IPP core] entity.delete batch=3 entity={}",
                    applied.to_bits()
                ),
                format!(
                    "[IPP core] entity.create batch=3 entity={}",
                    second.to_bits()
                ),
                format!(
                    "[IPP core] entity.delete batch=3 entity={}",
                    second.to_bits()
                ),
            ]
        );
        assert!(world.entities().is_empty());
        world.update_for_test(0.0).unwrap();
        run(&mut world, 4, vec![]);
        assert!(take().is_empty());
        configure(Level::Off, None);
    }

    #[test]
    fn direct_core_enqueue_budget_rejection_warns_without_effects() {
        configure(Level::Warn, Some(sink));
        let mut world_host = crate::support::task_scheduler::host();
        let world_id = world_host
            .create_world(
                ipp_core::WorldLimits {
                    max_operations: 256,
                    ..Default::default()
                },
                &[],
            )
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let operations = (0..257).map(create).collect();
        assert!(
            world
                .enqueue(Batch {
                    id: 42,
                    operations
                })
                .is_err()
        );
        assert_eq!(
            take(),
            vec![(
                Level::Warn,
                "[IPP core] batch.reject batch=42 operations=257 reason=enqueue_budget".into()
            )]
        );
        assert!(world.update_for_test(0.0).unwrap().outcomes.is_empty());
        assert!(world.entities().is_empty());
        assert!(take().is_empty());
        configure(Level::Off, None);
    }

    #[test]
    fn rejected_system_commands_warn_without_replies_or_committed_effect_logs() {
        use ipp_core::{CameraMotion, EntityId, RenderStatePatch};

        configure(Level::Debug, Some(sink));
        let mut world_host = crate::support::task_scheduler::host();
        let world_id = world_host
            .create_world(
                ipp_core::WorldLimits::default(),
                &crate::support::selection::select(&[
                    crate::support::selection::CAMERA,
                    crate::support::selection::RENDER,
                ]),
            )
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        world
            .enqueue_camera_activate(EntityId::from_bits(u64::MAX))
            .unwrap();
        world
            .enqueue_camera_navigate(CameraMotion::Zoom {
                amount: 1.0,
            })
            .unwrap();
        world
            .enqueue_render_state_update(RenderStatePatch {
                show_all_debug_geometries: Some(true),
                debug_geometry_color: Some([f32::NAN, 0.0, 0.0]),
                ambient_light: None,
            })
            .unwrap();
        let report = world.update_for_test(0.0).unwrap();
        assert!(report.outcomes.is_empty());
        assert!(report.camera_state_changes.is_empty());
        assert!(report.render_state_changes.is_empty());
        assert_eq!(world.active_camera(), None);
        assert!(!world.render_state().unwrap().show_all_debug_geometries);
        let lines = take();
        assert_eq!(lines.len(), 3);
        assert!(
            lines
                .iter()
                .all(|(level, line)| *level == Level::Warn && line.contains(".reject"))
        );
        assert!(lines[0].1.contains("InvalidEntity"));
        assert!(lines[1].1.contains("NoActiveCamera"));
        assert!(lines[2].1.contains("InvalidValue"));
        world
            .enqueue_render_state_update(RenderStatePatch::default())
            .unwrap();
        world.update_for_test(0.0).unwrap();
        assert!(take().is_empty());
        configure(Level::Off, None);
    }
}
