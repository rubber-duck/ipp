use super::*;

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn system_slots_follow_identity_not_selected_schedule_position() {
    let mut profiles = SystemProfileRegistry::new();
    let one = profiles.register("profile.test.a", 11, 0);
    let other = profiles.register("profile.test.b", 11, 0);
    let different_composition = profiles.register("profile.test.a", 12, 0);
    assert_eq!(profiles.register("profile.test.a", 11, 0), one);
    assert_ne!(one, different_composition);
    assert_ne!(one, other);
    assert_eq!(profiles.entries[one].composition, 11);
    assert_eq!(profiles.entries[different_composition].composition, 12);
}

#[test]
fn growing_counters_address_every_reserved_index_across_segments() {
    let counters = GrowingCounters::new();
    assert!(counters.get(0).is_none());

    let len = FIRST_SEGMENT * 7 + 1;
    counters.reserve(len);
    for index in [
        0,
        FIRST_SEGMENT - 1,
        FIRST_SEGMENT,
        FIRST_SEGMENT * 3,
        len - 1,
    ] {
        counters
            .get(index)
            .unwrap()
            .store(index as u64 + 1, Relaxed);
    }
    for index in [
        0,
        FIRST_SEGMENT - 1,
        FIRST_SEGMENT,
        FIRST_SEGMENT * 3,
        len - 1,
    ] {
        assert_eq!(counters.get(index).unwrap().load(Relaxed), index as u64 + 1);
    }
    assert_eq!(GrowingCounters::locate(FIRST_SEGMENT * 3), (2, 0));

    counters.clear();
    assert_eq!(counters.get(len - 1).unwrap().load(Relaxed), 0);
}

#[test]
fn systems_registered_after_many_others_are_still_timed() {
    let _serial = SERIAL.lock().unwrap();
    let composition = u64::MAX - 0x5eed;
    // More keys than the former fixed 30-slot table, all in one composition.
    let names: Vec<&'static str> = (0..40)
        .map(|index| &*Box::leak(format!("profile.test.growth.{index:02}").into_boxed_str()))
        .collect();
    let profiles: Vec<_> = names
        .iter()
        .map(|name| register_system(name, composition, 0))
        .collect();
    assert!(system_count() >= names.len());

    let last = *profiles.last().unwrap();
    assert_eq!(system_name(last), names[names.len() - 1]);
    assert_eq!(system_composition(last), composition);

    reset(true);
    for &profile in &profiles {
        drop(Stage::system(Some(profile), 3, "profile.test.growth"));
    }
    drop(Stage::system(Some(last), 5, names[names.len() - 1]));
    let observe = (last * SYSTEM_PHASES + 5) * 4;
    let category = (CATEGORIES + last * SYSTEM_PHASES + 5) * 2;
    let timed: Vec<_> = profiles
        .iter()
        .map(|profile| counter((profile * SYSTEM_PHASES + 3) * 4))
        .collect();
    ENABLED.store(false, Relaxed);

    assert!(timed.iter().all(|&calls| calls >= 1), "{timed:?}");
    assert!(counter(observe) >= 1);
    assert!(observe < counter_count());
    assert!(category < category_count() * 2);
    assert_eq!(category_name(category / 2), names[names.len() - 1]);
}
#[test]
fn world_contexts_retention_reuse_nested_allocations_and_reset() {
    let _serial = SERIAL.lock().unwrap();
    let world = register_world(7001, 1, 10001, 12);
    let other = register_world(7001, 2, 10002, 12);
    let first = register_system("profile.context.test", 12, world);
    let second = register_system("profile.context.test", 12, other);
    assert_ne!(first, second);
    assert_ne!(system_context(first), system_context(second));
    let stage_slot = first * SYSTEM_PHASES + 3;
    let phase_context = PROFILE_CONTEXTS.get(stage_slot).unwrap().load(Relaxed) as usize;

    reset(true);
    let named_handle;
    {
        let _world = ContextScope::world(world);
        let _stage = Stage::system(Some(first), 3, "profile.context.test");
        let _commit = ContextScope::world(world);
        assert_eq!(ACTIVE_CONTEXT.load(Relaxed), phase_context);
        let _fixed = Stage::fixed(FixedStage::CommitStorage);
        let inner = AllocationScope::new(200, "world.commit");
        named_handle = ACTIVE_CATEGORY.load(Relaxed);
        count(17);
        drop(inner);
        count(23);
    }
    pause();
    assert_eq!(allocations(), (2, 40));
    assert_eq!(counter(stage_slot * 4), 1);
    assert_eq!(world_context(phase_context).incarnation, 10001);
    assert_eq!(
        world_context(phase_context).phase,
        Some(ProfilePhase::Evaluate)
    );
    assert_eq!(
        active_category_counter(named_handle, 0)
            .unwrap()
            .load(Relaxed),
        1
    );
    let fixed_ordinal = phase_context * FixedStage::ALL.len() + FixedStage::CommitStorage as usize;
    assert_eq!(
        FIXED_COUNTERS.get(fixed_ordinal * 4).unwrap().load(Relaxed),
        1
    );
    assert_eq!(world_context(phase_context).system, "profile.context.test");
    assert_eq!(ACTIVE_CONTEXT.load(Relaxed), 0);

    reset(true);
    let stale = Stage::system(Some(first), 3, "profile.context.test");
    count(1);
    reset(true);
    let fresh = AllocationScope::new(200, "world.commit");
    let fresh_handle = ACTIVE_CATEGORY.load(Relaxed);
    drop(stale);
    assert_eq!(ACTIVE_CATEGORY.load(Relaxed), fresh_handle);
    assert!(std::panic::catch_unwind(release_capture).is_err());
    drop(fresh);
    assert_eq!(counter(stage_slot * 4), 0);
    assert_eq!(ACTIVE_CATEGORY.load(Relaxed), 0);
    assert_eq!(ACTIVE_CONTEXT.load(Relaxed), 0);
    pause();

    retire_world(world);
    assert_eq!(system_context(first).incarnation, 10001);
    let live = system_context(second);
    release_capture();
    assert_eq!(system_context(second), live);
    assert_eq!(system_name(first), "");
    let replacement = register_world(7002, 1, 10003, 12);
    let replaced = register_system("profile.context.test", 12, replacement);
    assert!(!system_name(replaced).is_empty());
    assert_eq!(system_context(replaced).incarnation, 10003);
    assert_eq!(system_context(replaced).host, 7002);
    assert_eq!(
        CONTEXTS
            .lock()
            .unwrap()
            .iter()
            .filter(|entry| entry.occupied && entry.context.host == 7002)
            .count(),
        7
    );
    retire_world(replacement);
    retire_world(other);
    release_capture();
}

#[test]
fn release_rejects_live_guards_and_repeated_churn_reuses_capacity() {
    let _serial = SERIAL.lock().unwrap();
    reset(true);
    let guard = ContextScope::new(0);
    assert!(std::panic::catch_unwind(release_capture).is_err());
    drop(guard);
    release_capture();

    for generation in 0..100 {
        let world = register_world(8001, generation, 20000 + generation, 42);
        register_system("profile.churn", 42, world);
        assert_eq!(
            CONTEXTS
                .lock()
                .unwrap()
                .iter()
                .filter(|entry| entry.occupied && entry.context.host == 8001)
                .count(),
            7
        );
        retire_world(world);
        release_capture();
    }
}

#[test]
fn registry_growth_preserves_active_semantic_allocation_handles() {
    let _serial = SERIAL.lock().unwrap();
    let world = register_world(9001, 1, 30001, 92);
    let profile = register_system("profile.growing.context", 92, world);
    reset(true);
    let named_handle;
    let named_context;
    {
        let _system = Stage::system(Some(profile), 3, "profile.growing.context");
        let _fixed = Stage::fixed(FixedStage::CommitStorage);
        let _named = AllocationScope::new(200, "world.commit");
        count(31);
        let handle = ACTIVE_CATEGORY.load(Relaxed);
        named_handle = handle;
        named_context = ACTIVE_CONTEXT.load(Relaxed);
        let another = register_world(9001, 2, 30002, 93);
        register_system("profile.growing.other", 93, another);
        assert_eq!(ACTIVE_CATEGORY.load(Relaxed), handle);
        count(47);
        retire_world(another);
    }
    pause();
    let context = world_context(named_context);
    assert_eq!(context.incarnation, 30001);
    assert_eq!(context.phase, Some(ProfilePhase::Evaluate));
    assert_eq!(
        active_category_counter(named_handle, 0)
            .unwrap()
            .load(Relaxed),
        2
    );
    assert_eq!(
        active_category_counter(named_handle, 1)
            .unwrap()
            .load(Relaxed),
        78
    );
    let sum = CATEGORY_COUNTS
        .iter()
        .step_by(2)
        .map(|counter| counter.load(Relaxed))
        .sum::<u64>()
        + [&SYSTEM_CATEGORY_COUNTS, &CONTEXT_CATEGORY_COUNTS]
            .into_iter()
            .map(|counts| {
                counts
                    .segments
                    .iter()
                    .filter_map(OnceLock::get)
                    .map(|segment| {
                        segment
                            .iter()
                            .step_by(2)
                            .map(|counter| counter.load(Relaxed))
                            .sum::<u64>()
                    })
                    .sum::<u64>()
            })
            .sum::<u64>();
    assert_eq!(sum, allocations().0);
    retire_world(world);
    release_capture();
}

#[test]
fn actual_host_world_lifetimes_keep_separate_profiles_after_destruction() {
    let _serial = SERIAL.lock().unwrap();
    let mut host = crate::HostRuntime::new();
    let selected: Vec<_> = host.system_ids().collect();
    let first = host.create_world(Default::default(), &selected).unwrap();
    let second = host.create_world(Default::default(), &selected).unwrap();
    let first_incarnation = host.world_ref(first).unwrap().incarnation();
    let second_incarnation = host.world_ref(second).unwrap().incarnation();
    reset(true);
    host.frame(0.0).unwrap();
    pause();
    let find = |incarnation| {
        (0..system_count())
            .find(|&index| {
                let context = system_context(index);
                context.host == host.identity()
                    && context.incarnation == incarnation
                    && context.system == selected[0].0
            })
            .unwrap()
    };
    let first_profile = find(first_incarnation);
    let second_profile = find(second_incarnation);
    assert_ne!(first_profile, second_profile);
    assert_eq!(counter(first_profile * SYSTEM_PHASES * 4), 1);
    assert_eq!(counter(second_profile * SYSTEM_PHASES * 4), 1);
    assert!(host.destroy_world(first));
    assert_eq!(system_context(first_profile).incarnation, first_incarnation);
    release_capture();
    assert_eq!(system_name(first_profile), "");
    assert_eq!(
        system_context(second_profile).incarnation,
        second_incarnation
    );
    let third = host.create_world(Default::default(), &selected).unwrap();
    assert_ne!(
        host.world_ref(third).unwrap().incarnation(),
        first_incarnation
    );
    drop(host);
    release_capture();
}

#[test]
fn shared_scopes_without_a_world_keep_explicit_unassigned_identity() {
    let _serial = SERIAL.lock().unwrap();
    reset(true);
    let handle;
    {
        let _fixed = Stage::fixed(FixedStage::CommitValidate);
        let _allocation = AllocationScope::new(211, "assets.poll");
        handle = ACTIVE_CATEGORY.load(Relaxed);
        count(11);
    }
    pause();
    assert_eq!(world_context(0), ProfileContext::default());
    assert_eq!(active_category_counter(handle, 0).unwrap().load(Relaxed), 1);
    assert_eq!(
        FIXED_COUNTERS
            .get(FixedStage::CommitValidate as usize * 4)
            .unwrap()
            .load(Relaxed),
        1
    );
    release_capture();
}

#[test]
fn captured_thread_and_host_exclude_foreign_scopes_and_allocations() {
    let _serial = SERIAL.lock().unwrap();
    let owner = register_world(8001, 1, 18001, 12);
    let foreign = register_world(8002, 1, 18002, 12);
    reset_for_host(true, 8001);
    {
        let _owner = ContextScope::new(owner);
        count(11);
        {
            let _foreign = ContextScope::new(foreign);
            count(99);
            drop(Stage::fixed(FixedStage::CommitStorage));
        }
        count(13);
        std::thread::spawn(move || {
            let _foreign = ContextScope::new(owner);
            count(101);
            drop(Stage::fixed(FixedStage::CommitStorage));
        })
        .join()
        .unwrap();
    }
    pause();
    assert_eq!(allocations(), (2, 24));
    let totals = (0..category_count())
        .map(|category| {
            (
                category_counter(category * 2),
                category_counter(category * 2 + 1),
            )
        })
        .fold((0, 0), |total, value| {
            (total.0 + value.0, total.1 + value.1)
        });
    assert_eq!(totals, allocations());
    retire_world(owner);
    retire_world(foreign);
    release_capture();
}

#[test]
fn complete_trace_spans_are_bounded_and_independent_of_counters() {
    let _serial = SERIAL.lock().unwrap();
    release_capture();
    crate::profiling_trace::prepare(2).unwrap();
    reset_for_host(false, 9001);
    enable_trace();
    let context = register_world(9001, 9002, 9003, 9004);
    {
        let _world = ContextScope::new(context);
        let _frame = crate::profiling_trace::frame();
        let outer = Stage::fixed(FixedStage::CommitStorage);
        drop(Stage::fixed(FixedStage::CommitValidate));
        drop(outer);
        drop(Stage::fixed(FixedStage::CommitStorage));
    }
    pause();
    let mut spans = Vec::new();
    crate::profiling_trace::visit(|span| {
        spans.push(*span);
        true
    });
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].sequence, 1);
    assert_eq!(spans[1].sequence, 2);
    assert!(spans[0].start <= spans[1].start && spans[0].end >= spans[1].end);
    assert!(spans.iter().all(|span| span.context.host == 9001
        && span.context.world == 9002
        && span.context.incarnation == 9003
        && span.context.composition == 9004
        && span.host_frame == Some(1)
        && span.thread == 1));
    assert_eq!(crate::profiling_trace::retention().1, 1);
    assert_eq!(allocations(), (0, 0));
    assert_eq!(
        FIXED_COUNTERS
            .get(context * FixedStage::ALL.len() * 4)
            .unwrap()
            .load(Relaxed),
        0
    );
    retire_world(context);
    release_capture();
    assert_eq!(crate::profiling_trace::retention(), (0, 0, 0));
}

#[test]
fn trace_generation_reset_clears_history_and_rejects_live_trace_only_guards() {
    let _serial = SERIAL.lock().unwrap();
    release_capture();
    crate::profiling_trace::prepare(2).unwrap();
    reset(false);
    enable_trace();
    drop(Stage::fixed(FixedStage::CommitStorage));
    let stale = Stage::fixed(FixedStage::CommitValidate);
    reset(false);
    enable_trace();
    let current = Stage::fixed(FixedStage::CommitStorage);
    drop(stale);
    assert!(std::panic::catch_unwind(release_capture).is_err());
    drop(current);
    pause();
    let mut spans = Vec::new();
    crate::profiling_trace::visit(|span| {
        spans.push(*span);
        true
    });
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].sequence, 1);
    assert_eq!(spans[0].name, FixedStage::CommitStorage.name());
    assert_eq!(spans[0].host_frame, None);
    release_capture();
}
