use super::*;

fn outputs() -> [OutputRef; 3] {
    let mut host = ipp_core::HostRuntime::new();
    std::array::from_fn(|_| {
        let world = host.create_world(Default::default(), &[]).unwrap();
        OutputRef::canvas(host.world_ref(world).unwrap())
    })
}

fn request(output: OutputRef, score: f64) -> ImageRequest {
    ImageRequest {
        output,
        score,
        size: [512; 2],
        count: 1,
    }
}

#[test]
fn coverage_ranking_can_replace_previous_winner_and_apply_priority_hint() {
    let [a, b, _] = outputs();
    let budget = 512 * 512 * 4 + 256 * 256 * 4;
    let first = plan(vec![request(a, 0.2), request(b, 0.4)], budget);
    assert_eq!(first[&b], 512 * 512 * 4);
    assert_eq!(first[&a], 256 * 256 * 4);
    let moved = plan(vec![request(b, 0.2), request(a, 0.4)], budget);
    assert_eq!(moved[&a], first[&b]);
    let hinted = plan(vec![request(a, 0.2 * 3.0), request(b, 0.4)], budget);
    assert_eq!(hinted, moved);
    let tied = plan(vec![request(b, 0.4), request(a, 0.4)], budget);
    assert_eq!(
        tied, moved,
        "equal coverage has stable output identity order"
    );
}

#[test]
fn separated_layers_share_one_rank_but_account_for_every_image() {
    let [a, b, c] = outputs();
    let mut layered = request(a, 1.0);
    layered.count = 3;
    let allocations = plan(vec![layered, request(b, 0.2), request(c, 0.1)], 1 << 20);
    assert_eq!(allocations[&a], 3 * 256 * 256 * 4);
    assert!(allocations[&b] >= 4 && allocations[&c] >= 4);
    assert!(allocations.values().sum::<usize>() <= 1 << 20);
    assert_eq!(plan(vec![request(a, 1.0)], 0)[&a], 0);
}
