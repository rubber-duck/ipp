use super::*;
use crate::{
    DynamicPropertyKind, DynamicValue, WorldId,
    expressions::{
        BinaryOperator, ExpressionInput, ExpressionInputError, ExpressionNode, ExpressionResult,
    },
    services::{
        asset_management::{
            AssetKey, AssetLifecycleKind, AssetLoadStatus, AssetManagementService,
            AssetReleaseKind, AssetSource, AssetUploadIdentity, service::AssetDemandSelection,
        },
        io::{IoService, MemoryIoSource},
    },
};
use std::collections::BTreeSet;

fn declaration() -> ExpressionDeclaration {
    ExpressionDeclaration {
        inputs: vec![ExpressionInput {
            name: "sample".into(),
            kind: DynamicPropertyKind::F32,
        }],
        nodes: vec![
            ExpressionNode::Input(0),
            ExpressionNode::Constant(DynamicValue::F32(2.0)),
            ExpressionNode::Binary {
                operator: BinaryOperator::Multiply,
                left: 0,
                right: 1,
            },
        ],
        output: 2,
    }
}

fn source(uri: &str) -> AssetSource {
    AssetSource {
        kind: EXPRESSION_TYPE,
        uri: uri.into(),
        variant: 0,
    }
}

fn fixture() -> (AssetManagementService, IoService) {
    let assets = AssetManagementService::new();
    let mut io = IoService::new();
    assets.install_io_sources(&mut io).unwrap();
    (assets, io)
}

fn demand(
    assets: &mut AssetManagementService,
    world: WorldId,
    system: &'static str,
    source: &AssetSource,
) {
    let selection = AssetDemandSelection::new(source.kind, &source.uri, source.variant);
    assets
        .validate_additional_users(world, [selection.clone()])
        .unwrap();
    assets.update_system_users(world, system, BTreeSet::from([selection]));
}

fn clear_demand(assets: &mut AssetManagementService, world: WorldId, system: &'static str) {
    assets.update_system_users(world, system, BTreeSet::new());
}

fn evaluate(assets: &AssetManagementService, key: AssetKey, input: f32) -> f32 {
    let plan = assets.get_typed::<ExpressionAsset>(key).unwrap().prepared();
    let value = DynamicValue::F32(input);
    let mut scratch = plan.scratch();
    match plan.evaluate(&mut scratch, &[Some(&value)]).unwrap() {
        ExpressionResult::Valid(DynamicValue::F32(value)) => *value,
        other => panic!("unexpected result: {other:?}"),
    }
}

fn finish_pending(assets: &mut AssetManagementService) {
    for (_, event) in assets.pending_releases() {
        assert!(assets.finish_release(&event));
    }
}

#[test]
fn compiled_cpu_loader_supports_shared_ordinary_system_demand_and_idle_eviction() {
    assert_eq!(EXPRESSION_TYPE, AssetTypeId(19));
    let (mut assets, mut io) = fixture();
    let memory = MemoryIoSource::default();
    let source = source("expression:shared");
    let bytes = declaration().encode().unwrap();
    let source_bytes = bytes.len() as u64;
    memory.insert(source.uri.to_string(), bytes).unwrap();
    io.register("expression:", memory).unwrap();
    demand(&mut assets, WorldId(1), "binding", &source);
    demand(&mut assets, WorldId(1), "driver", &source);
    demand(&mut assets, WorldId(2), "driver", &source);
    let key = assets.find(&source).unwrap();
    assert_eq!(
        assets.get(key).unwrap().status(),
        &AssetLoadStatus::Unloaded
    );

    assets.poll_evaluation_assets(&mut io);
    let asset = assets.get_typed::<ExpressionAsset>(key).unwrap();
    assert_eq!(asset.declaration(), &declaration());
    assert_eq!(asset.prepared().input_slots(), declaration().inputs);
    assert_eq!(asset.prepared().output_kind(), DynamicPropertyKind::F32);
    let resident = asset.resident_bytes();
    assert!(resident > 0);
    let representation = assets.get(key).unwrap().representation();
    assert!(representation.decoded);
    assert_eq!(representation.graphics_ready, None);
    assert_eq!(representation.graphics_bytes, None);
    assert_eq!(representation.source_bytes, source_bytes);
    assert_eq!(representation.resident_bytes, resident as u64);
    assert_eq!(assets.resident_bytes(), resident);
    assert_eq!(evaluate(&assets, key, 3.0), 6.0);
    assert_eq!(evaluate(&assets, key, -4.0), -8.0);
    assert_eq!(assets.snapshots(WorldId(1))[0].id, key.to_u64());
    assert_eq!(assets.snapshots(WorldId(2))[0].id, key.to_u64());
    let events = assets.take_events().unwrap();
    assert_eq!(events.first().unwrap().status, AssetLoadStatus::Start);
    assert_eq!(events.last().unwrap().status, AssetLoadStatus::Loaded);

    clear_demand(&mut assets, WorldId(1), "binding");
    assert_eq!(evaluate(&assets, key, 5.0), 10.0);
    assets.release_world(WorldId(1));
    assert_eq!(assets.idle_resident_bytes(), 0);
    assert_eq!(evaluate(&assets, key, 6.0), 12.0);
    clear_demand(&mut assets, WorldId(2), "driver");
    assert_eq!(assets.idle_resident_bytes(), resident);
    demand(&mut assets, WorldId(2), "driver", &source);
    assets.poll_evaluation_assets(&mut io);
    assert_eq!(assets.idle_resident_bytes(), 0);
    assert!(assets.take_events().unwrap().is_empty());

    clear_demand(&mut assets, WorldId(2), "driver");
    assets.set_idle_resident_bytes_target(0);
    assert!(assets.get(key).is_none());
    assert_eq!(assets.resident_bytes(), 0);
    demand(&mut assets, WorldId(2), "driver", &source);
    let replacement = assets.find(&source).unwrap();
    assert_eq!(replacement.slot, key.slot);
    assert_ne!(replacement.generation, key.generation);
    assets.poll_evaluation_assets(&mut io);
    assert_eq!(evaluate(&assets, replacement, 7.0), 14.0);
}

#[test]
fn explicit_unload_waits_for_invalidation_and_recovers_with_a_fresh_plan() {
    let (mut assets, mut io) = fixture();
    let memory = MemoryIoSource::default();
    let source = source("expression:recover");
    let bytes = declaration().encode().unwrap();
    memory
        .insert(source.uri.to_string(), bytes.clone())
        .unwrap();
    io.register("expression:", memory).unwrap();
    assets.require_lifecycle_barrier();
    demand(&mut assets, WorldId(3), "driver", &source);
    assets.poll_evaluation_assets(&mut io);
    let key = assets.find(&source).unwrap();
    let old_plan = assets
        .get_typed::<ExpressionAsset>(key)
        .unwrap()
        .prepared()
        .clone();
    let mut old_scratch = old_plan.scratch();
    assets.take_lifecycle_events();

    assets.unload(key);
    let pending = assets.pending_releases();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].0, AssetReleaseKind::Unload);
    assert_eq!(pending[0].1.kind, AssetLifecycleKind::StatusChanged);
    assert_eq!(evaluate(&assets, key, 9.0), 18.0);
    assets.poll_evaluation_assets(&mut io);
    assert_eq!(assets.get(key).unwrap().status(), &AssetLoadStatus::Loaded);
    // The consumer has now invalidated its plan binding; the barrier may commit.
    drop(old_plan);
    assert!(assets.finish_release(&pending[0].1));
    assert!(!assets.finish_release(&pending[0].1));
    assert!(assets.get_typed::<ExpressionAsset>(key).is_none());
    assert_eq!(assets.resident_bytes(), 0);
    assert!(assets.get(key).unwrap().requires_recovery());
    assert!(assets.take_lifecycle_events().iter().any(|event| {
        event.key == key
            && event.status == AssetLoadStatus::Unloaded
            && !event.representation.decoded
    }));

    assets.poll_evaluation_assets(&mut io);
    assert_eq!(assets.find(&source), Some(key));
    assert_eq!(
        assets.get(key).unwrap().stats().source_bytes,
        bytes.len() as u64
    );
    assert_eq!(evaluate(&assets, key, 10.0), 20.0);
    let plan = assets.get_typed::<ExpressionAsset>(key).unwrap().prepared();
    assert_eq!(
        plan.evaluate(&mut old_scratch, &[None]),
        Err(ExpressionInputError::StaleScratch)
    );
    assert!(
        assets
            .take_lifecycle_events()
            .iter()
            .any(|event| event.status == AssetLoadStatus::Loaded)
    );
}

#[test]
fn producer_release_preserves_demanded_recovery_and_world_source_isolation() {
    let (mut assets, mut io) = fixture();
    assets.set_idle_resident_bytes_target(0);
    let identity = AssetUploadIdentity {
        kind: EXPRESSION_TYPE,
        asset: 42,
        variant: 0,
    };
    let local = source("asset://19/42");
    let first = assets
        .upload_world(WorldId(4), identity, declaration().encode().unwrap())
        .unwrap();
    let mut other = declaration();
    other.nodes[1] = ExpressionNode::Constant(DynamicValue::F32(3.0));
    let second = assets
        .upload_world(WorldId(5), identity, other.encode().unwrap())
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(
        assets.get(first).unwrap().source().uri.as_ref(),
        "producer://4/19/42"
    );
    assert_eq!(
        assets.get(second).unwrap().source().uri.as_ref(),
        "producer://5/19/42"
    );
    assert!(
        assets
            .upload_world(WorldId(4), identity, other.encode().unwrap())
            .is_err()
    );
    demand(&mut assets, WorldId(4), "binding", &local);
    demand(&mut assets, WorldId(5), "driver", &local);
    assets.poll_evaluation_assets(&mut io);
    assert_eq!(evaluate(&assets, first, 7.0), 14.0);
    assert_eq!(evaluate(&assets, second, 7.0), 21.0);

    assets.release_upload(WorldId(4), identity);
    assert_eq!(io.list("asset-memory:").unwrap().len(), 2);
    assets.unload(first);
    assets.poll_evaluation_assets(&mut io);
    assert_eq!(evaluate(&assets, first, 8.0), 16.0);
    clear_demand(&mut assets, WorldId(4), "binding");
    assert!(assets.get(first).is_none());
    assert_eq!(io.list("asset-memory:").unwrap().len(), 1);
    assert_eq!(evaluate(&assets, second, 8.0), 24.0);
    assets.release_world(WorldId(5));
    assert!(assets.get(second).is_none());
    assert!(io.list("asset-memory:").unwrap().is_empty());
    assert_eq!(assets.resident_bytes(), 0);
}

#[test]
fn replacement_io_registration_cannot_retarget_recovery_content() {
    let (mut assets, mut io) = fixture();
    let source = source("expression:fenced");
    let memory = MemoryIoSource::default();
    memory
        .insert(source.uri.to_string(), declaration().encode().unwrap())
        .unwrap();
    io.register("expression:", memory).unwrap();
    demand(&mut assets, WorldId(6), "driver", &source);
    assets.poll_evaluation_assets(&mut io);
    let key = assets.find(&source).unwrap();
    assets.unload(key);
    assert!(io.unregister("expression:"));
    let replacement = MemoryIoSource::default();
    replacement
        .insert(source.uri.to_string(), declaration().encode().unwrap())
        .unwrap();
    io.register("expression:", replacement).unwrap();
    assets.poll_evaluation_assets(&mut io);

    assert_eq!(assets.find(&source), Some(key));
    assert_eq!(
        assets.get(key).unwrap().status(),
        &AssetLoadStatus::Failed("Immutable recovery source registration changed".into())
    );
    assert!(assets.get_typed::<ExpressionAsset>(key).is_none());
}

#[test]
fn malformed_and_semantically_invalid_loader_results_are_observable_and_never_decoded() {
    let valid = declaration().encode().unwrap();
    let mut wrong_type = valid.clone();
    // Single named F32 input follows the 20-byte header and u32/string name.
    wrong_type[20 + 4 + "sample".len()] = 4; // Bool conflicts with Multiply/F32.
    let mut trailing = valid.clone();
    trailing.push(0);
    let cases = [
        (b"bad!".to_vec(), "InvalidHeader"),
        (valid[..valid.len() - 1].to_vec(), "Truncated"),
        (wrong_type, "WrongType"),
        (trailing, "TrailingBytes"),
    ];
    for (bytes, diagnostic) in cases {
        let (mut assets, mut io) = fixture();
        io.register_stream("expression:").unwrap();
        assets.require_lifecycle_barrier();
        let source = source("expression:malformed");
        demand(&mut assets, WorldId(7), "driver", &source);
        let key = assets.find(&source).unwrap();
        assets.poll_evaluation_assets(&mut io);
        let request = io.take_requests().pop().unwrap();
        let middle = bytes.len() / 2;
        io.input_chunk(request.id, &bytes[..middle]).unwrap();
        assets.poll_evaluation_assets(&mut io);
        assert!(assets.get_typed::<ExpressionAsset>(key).is_none());
        io.input_chunk(request.id, &bytes[middle..]).unwrap();
        io.input_end(request.id, Ok(()));
        assets.poll_evaluation_assets(&mut io);

        let AssetLoadStatus::Failed(error) = assets.get(key).unwrap().status() else {
            panic!("expected failed decoding");
        };
        assert!(error.contains(diagnostic), "{error}");
        assert_eq!(
            assets.get(key).unwrap().stats().source_bytes,
            bytes.len() as u64
        );
        assert!(!assets.get(key).unwrap().representation().decoded);
        assert_eq!(assets.resident_bytes(), 0);
        let observed = assets.snapshots(WorldId(7));
        assert!(matches!(observed[0].status, AssetLoadStatus::Failed(_)));
        assert!(assets.take_lifecycle_events().iter().any(|event| matches!(&event.status, AssetLoadStatus::Failed(message) if message.contains(diagnostic))));

        // A loader refusal leaves the authored demand and stable identity intact.
        assets.unload(key);
        finish_pending(&mut assets);
        assets.poll_evaluation_assets(&mut io);
        let retry = io.take_requests().pop().unwrap();
        assert!(!retry.recovery);
        io.input_chunk(retry.id, &valid).unwrap();
        io.input_end(retry.id, Ok(()));
        assets.poll_evaluation_assets(&mut io);
        assert_eq!(assets.find(&source), Some(key));
        assert_eq!(evaluate(&assets, key, 11.0), 22.0);
    }
}

#[test]
fn streamed_input_limit_refuses_growth_before_eof_and_cancels_the_reader() {
    let (mut assets, mut io) = fixture();
    io.register_stream("expression:").unwrap();
    let source = source("expression:oversized");
    demand(&mut assets, WorldId(8), "binding", &source);
    assets.poll_evaluation_assets(&mut io);
    let key = assets.find(&source).unwrap();
    let request = io.take_requests().pop().unwrap();
    let chunk = [0; crate::services::io::STREAM_CAPACITY];
    for _ in 0..EXPRESSION_MAX_BYTES / chunk.len() {
        io.input_chunk(request.id, &chunk).unwrap();
        assets.poll_evaluation_assets(&mut io);
        assert!(!matches!(
            assets.get(key).unwrap().status(),
            AssetLoadStatus::Failed(_)
        ));
    }
    io.input_chunk(request.id, &[0]).unwrap();
    assets.poll_evaluation_assets(&mut io);

    assert_eq!(
        assets.get(key).unwrap().status(),
        &AssetLoadStatus::Failed("Asset input byte limit exceeded".into())
    );
    assert_eq!(
        assets.get(key).unwrap().stats().source_bytes,
        (EXPRESSION_MAX_BYTES + 1) as u64
    );
    assert_eq!(io.take_cancellations(), vec![request.id]);
    assert_eq!(io.input_chunk(request.id, &[1]), Ok(true));
    assert_eq!(io.input_bytes(), 0);
    assert!(assets.get_typed::<ExpressionAsset>(key).is_none());
    assert_eq!(assets.resident_bytes(), 0);
}
