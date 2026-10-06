use super::*;
use crate::world::lifecycle_diagnostics::{LifecycleDiagnosticQuery, LifecycleDiagnosticSample};
use ipp_core::systems::lifecycle_publisher::{LifecycleTargetWork, LifecycleWatchTraffic};

fn endpoint() -> LifecycleDiagnosticQuery {
    LifecycleDiagnosticQuery {
        world: crate::references::WorldReference {
            id: 3,
            incarnation: 5,
        },
        output: 9,
    }
}

fn world() -> ManifestValue {
    manifest_layout(
        "world-reference",
        [
            ("id", ManifestValue::U64(3)),
            ("incarnation", ManifestValue::U64(5)),
        ],
    )
}

pub(super) fn requests(covered: &mut BTreeSet<&'static str>) {
    let fixture = ManifestFixture::new(
        "request-lifecycle-diagnostics",
        [
            ("session", ManifestValue::U64(7)),
            ("request_id", ManifestValue::U64(2)),
            ("tag", ManifestValue::Tag("REQUEST_LIFECYCLE_DIAGNOSTICS")),
            ("world", world()),
            ("output", ManifestValue::U64(9)),
        ],
    );
    let bytes = encode_manifest_fixture(&fixture, covered);
    assert_eq!(
        decode_request(&bytes, 7).unwrap().body,
        RequestBody::LifecycleDiagnostics(endpoint())
    );
    for length in 0..bytes.len() {
        assert!(decode_request(&bytes[..length], 7).is_err());
    }
    for offset in [17, 25, 33] {
        let mut invalid = bytes.clone();
        invalid[offset..offset + 8].fill(0);
        assert!(decode_request(&invalid, 7).is_err());
    }
    assert!(decode_request(&bytes, 8).is_err());
    let mut trailing = bytes;
    trailing.push(0);
    assert!(decode_request(&trailing, 7).is_err());
}

pub(super) fn responses(covered: &mut BTreeSet<&'static str>) {
    let body = ResponseBody::LifecycleDiagnostics(LifecycleDiagnosticSample {
        endpoint: endpoint(),
        work: LifecycleTargetWork {
            lookups: u64::MAX,
            recipient_visits: 12,
            saturated: true,
        },
        traffic: LifecycleWatchTraffic {
            queued_events: 2,
            queued_bytes: 99,
            saturated: false,
        },
    });
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 2,
            tick: 0,
            body: body.clone(),
        },
        ManifestFixture::new(
            "response-lifecycle-diagnostics",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(2)),
                ("tick", ManifestValue::U64(0)),
                ("tag", ManifestValue::Tag("RESPONSE_LIFECYCLE_DIAGNOSTICS")),
                ("world", world()),
                ("output", ManifestValue::U64(9)),
                ("lookups", ManifestValue::U64(u64::MAX)),
                ("recipient_visits", ManifestValue::U64(12)),
                ("work_saturated", ManifestValue::Bool(true)),
                ("queued_events", ManifestValue::U64(2)),
                ("queued_bytes", ManifestValue::U64(99)),
                ("traffic_saturated", ManifestValue::Bool(false)),
            ],
        ),
        covered,
    );
    assert!(
        encode_response(&Response {
            session: 7,
            request_id: 2,
            tick: 1,
            body: body.clone()
        })
        .is_err()
    );
    assert!(
        encode_response(&Response {
            session: 7,
            request_id: 0,
            tick: 0,
            body
        })
        .is_err()
    );
}
