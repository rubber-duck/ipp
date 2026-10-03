use super::*;

fn key(entity: u64, component: u16) -> DriverKey {
    DriverKey(EntityId::from_bits(entity), component)
}

fn property(entity: u64, offset: u32) -> PropertyIdentity {
    PropertyIdentity {
        entity: EntityId::from_bits(entity),
        incarnation: 7,
        kind: crate::DynamicPropertyKind::F32,
        property: DriverProperty {
            component: 1,
            offset,
        },
    }
}

#[test]
fn multiinput_scc_suppresses_only_members_and_orders_downstream() {
    let driver = |entity, sources: Vec<_>| DriverDependencies {
        key: key(entity, 102),
        target: property(entity, 0),
        sources,
    };
    let drivers = [
        driver(1, vec![property(2, 0), property(3, 0)]),
        driver(2, vec![property(1, 0)]),
        driver(3, vec![]),
        driver(4, vec![property(1, 0), property(3, 0)]),
        driver(5, vec![property(4, 0)]),
    ];
    let (order, cycle) = super::order(&drivers);
    assert_eq!(cycle, BTreeSet::from([key(1, 102), key(2, 102)]));
    assert_eq!(order, [key(3, 102), key(4, 102), key(5, 102)]);
    let corrected = [
        driver(1, vec![property(3, 0)]),
        driver(2, vec![property(1, 0)]),
        driver(3, vec![]),
        driver(4, vec![property(1, 0), property(2, 0)]),
        driver(5, vec![property(4, 0)]),
    ];
    let (order, cycle) = super::order(&corrected);
    assert!(cycle.is_empty());
    assert_eq!(
        order,
        [
            key(3, 102),
            key(1, 102),
            key(2, 102),
            key(4, 102),
            key(5, 102)
        ]
    );
}

#[test]
fn property_overlap_incarnation_and_writer_ties_are_exact() {
    let drivers = [
        DriverDependencies {
            key: key(1, 2),
            target: property(1, 0),
            sources: vec![],
        },
        DriverDependencies {
            key: key(1, 102),
            target: property(1, 0),
            sources: vec![property(1, 4)],
        },
        DriverDependencies {
            key: key(2, 102),
            target: property(2, 0),
            sources: vec![property(1, 0)],
        },
    ];
    let (order, cycle) = super::order(&drivers);
    assert!(cycle.is_empty());
    assert_eq!(order, [key(1, 2), key(1, 102), key(2, 102)]);
    let mut other_incarnation = property(1, 0);
    other_incarnation.incarnation = 8;
    let no_cycle = [DriverDependencies {
        key: key(1, 102),
        target: property(1, 0),
        sources: vec![other_incarnation],
    }];
    assert_eq!(
        super::order(&no_cycle),
        (vec![key(1, 102)], BTreeSet::new())
    );
}

#[test]
fn large_chain_preparation_uses_no_recursive_call_stack() {
    let drivers: Vec<_> = (1..=10_000)
        .map(|entity| DriverDependencies {
            key: key(entity, 102),
            target: property(entity, 0),
            sources: vec![property(entity - 1, 0)],
        })
        .collect();
    let (order, cycle) = super::order(&drivers);
    assert!(cycle.is_empty());
    assert_eq!(order.len(), 10_000);
    assert_eq!(order[0], key(1, 102));
    assert_eq!(order[9999], key(10_000, 102));
}
