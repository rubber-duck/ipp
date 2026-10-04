//! Availability/scheduling invariants supplement the actual EGL/WebGL readback harness.

mod support;

use ipp_core::{
    TextureAsset,
    services::asset_management::{
        AssetKey, AssetUploadIdentity,
        export::{AssetExportFormat, AssetOutputObserver},
    },
};
use ipp_render_gl::{RenderDevice, RenderService};
use std::{
    cell::Cell,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};
use support::{DeviceState, TestDevice};

#[derive(Default)]
struct Gate {
    ready: AtomicBool,
    waiter: Mutex<Option<Waker>>,
}

impl Gate {
    fn release(&self) {
        self.ready.store(true, Ordering::SeqCst);
        if let Some(waiter) = self.waiter.lock().unwrap().take() {
            waiter.wake();
        }
    }
}

struct Delay(Arc<Gate>);

impl Future for Delay {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        *self.0.waiter.lock().unwrap() = Some(cx.waker().clone());
        if self.0.ready.load(Ordering::SeqCst) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct Account {
    bytes: Cell<usize>,
    peak: Cell<usize>,
    cancelled: Cell<bool>,
}

impl AssetOutputObserver for Account {
    fn reserve(&self, bytes: usize) -> Result<(), String> {
        self.check()?;
        self.bytes.set(bytes);
        self.peak.set(self.peak.get().max(bytes));
        Ok(())
    }

    fn check(&self) -> Result<(), String> {
        if self.cancelled.get() {
            Err("Pressure revoked output".into())
        } else {
            Ok(())
        }
    }
}

struct Fixture {
    host: ipp_core::HostRuntime,
    renderer: RenderService<TestDevice>,
    state: Rc<DeviceState>,
    key: AssetKey,
    account: Rc<Account>,
    gate: Arc<Gate>,
    pixels: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let mut host = support::task_scheduler::host();
        let state = Rc::new(DeviceState::default());
        state.texture_readback_enabled.set(true);
        let mut renderer = RenderService::new(TestDevice(state.clone())).unwrap();
        renderer.install(&mut host).unwrap();
        let gate = Arc::new(Gate::default());
        let wake = gate.clone();
        renderer.set_asset_export_delay(Rc::new(move |_| Box::pin(Delay(wake.clone()))));
        let pixels = vec![
            17, 89, 231, 41, 199, 51, 103, 255, 11, 208, 75, 0, 93, 14, 181, 117, 240, 123, 7, 201,
            62, 154, 219, 63,
        ];
        let mut encoded = b"IPPT".to_vec();
        for word in [3u32, 3, 2] {
            encoded.extend(word.to_le_bytes());
        }
        encoded.extend(&pixels);
        let key = host
            .asset_resources_mut()
            .upload(
                AssetUploadIdentity {
                    kind: ipp_core::TEXTURE_TYPE,
                    asset: 7,
                    variant: 0,
                },
                encoded,
            )
            .unwrap();
        support::progress_assets(&mut host);
        assert_eq!(
            host.asset_resources().get(key).unwrap().graphics_ready(),
            Some(true)
        );
        Self {
            host,
            renderer,
            state,
            key,
            account: Rc::new(Account::default()),
            gate,
            pixels,
        }
    }

    fn export(&self) -> ipp_core::services::asset_management::export::AssetExportFuture {
        self.renderer
            .export_asset(
                self.host.asset_resources().get(self.key).unwrap(),
                AssetExportFormat::TextureV3,
                self.account.clone(),
            )
            .unwrap()
    }
}

#[test]
fn owned_export_defers_device_work_until_poll_and_awaits_fence_with_a_real_waker() {
    let fixture = Fixture::new();
    let mut future = fixture.export();
    assert_eq!(fixture.state.texture_readback_begins.get(), 0);
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(future.as_mut().poll(&mut cx).is_pending());
    assert_eq!(fixture.account.bytes.get(), fixture.pixels.len());
    assert_eq!(fixture.state.texture_readback_begins.get(), 1);
    fixture.state.texture_readback_ready.set(true);
    fixture.gate.release();
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    let Poll::Ready(Ok(encoded)) = future.as_mut().poll(&mut cx) else {
        panic!("released fence did not complete")
    };
    let texture = TextureAsset::decode(&encoded).unwrap();
    assert_eq!(texture.pixels(), fixture.pixels);
    assert_eq!(
        fixture.account.peak.get(),
        (64 << 10) + fixture.pixels.len()
    );
    assert_eq!(fixture.account.bytes.get(), 64 << 10);
    assert_eq!(fixture.state.texture_readback_deletes.get(), 1);
}

#[test]
fn signaled_fence_completes_without_a_platform_delay_by_default() {
    let fixture = Fixture::new();
    fixture.state.texture_readback_ready.set(true);
    let mut future = fixture.export();
    let waker = Waker::from(Arc::new(WakeCount::default()));
    let Poll::Ready(Ok(encoded)) = future.as_mut().poll(&mut Context::from_waker(&waker)) else {
        panic!("An already signaled fence should complete in the current service turn");
    };
    assert_eq!(
        TextureAsset::decode(&encoded).unwrap().pixels(),
        fixture.pixels
    );
    assert!(fixture.gate.waiter.lock().unwrap().is_none());
}

#[cfg(feature = "instrumentation")]
#[test]
fn explicit_staging_gate_retains_a_real_stage_until_woken_or_cancelled() {
    let mut fixture = Fixture::new();
    fixture.state.texture_readback_ready.set(true);
    fixture.renderer.set_asset_export_staging_gate(true);
    let mut future = fixture.export();
    // Admission captures the gate: releasing it for later requests must not
    // change this operation's pending, already owned staging lifetime.
    fixture.renderer.set_asset_export_staging_gate(false);
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(future.as_mut().poll(&mut cx).is_pending());
    assert_eq!(fixture.state.texture_readback_begins.get(), 1);
    assert_eq!(fixture.state.texture_readback_deletes.get(), 0);
    assert_eq!(fixture.account.bytes.get(), fixture.pixels.len());
    fixture.gate.release();
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(_))));
    assert_eq!(fixture.state.texture_readback_deletes.get(), 1);

    let mut fixture = Fixture::new();
    fixture.state.texture_readback_ready.set(true);
    fixture.renderer.set_asset_export_staging_gate(true);
    let mut future = fixture.export();
    assert!(future.as_mut().poll(&mut cx).is_pending());
    drop(future);
    assert_eq!(fixture.state.texture_readback_deletes.get(), 1);
    assert_eq!(fixture.account.bytes.get(), 0);
}

#[test]
fn unload_before_first_poll_never_binds_or_stages_the_captured_texture_identifier() {
    let mut fixture = Fixture::new();
    let mut future = fixture.export();
    fixture.host.asset_resources_mut().unload(fixture.key);
    fixture.host.flush_resource_lifecycle();
    let waker = Waker::from(Arc::new(WakeCount::default()));
    assert!(matches!(
        future.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Err(_))
    ));
    assert_eq!(fixture.state.texture_readback_begins.get(), 0);
}

#[test]
fn cancellation_and_unload_retire_only_the_live_creating_stage() {
    let mut fixture = Fixture::new();
    let mut future = fixture.export();
    let waker = Waker::from(Arc::new(WakeCount::default()));
    let mut cx = Context::from_waker(&waker);
    assert!(future.as_mut().poll(&mut cx).is_pending());
    fixture.host.asset_resources_mut().unload(fixture.key);
    fixture.host.flush_resource_lifecycle();
    fixture.gate.release();
    assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Err(_))));
    assert_eq!(fixture.state.texture_readback_deletes.get(), 1);
    assert!(fixture.state.texture_readback_stages.borrow().is_empty());

    let fixture = Fixture::new();
    let mut future = fixture.export();
    assert!(future.as_mut().poll(&mut cx).is_pending());
    drop(future);
    assert_eq!(fixture.state.texture_readback_deletes.get(), 1);
    assert_eq!(fixture.account.bytes.get(), 0);
}

#[test]
fn obsolete_stage_drop_cannot_delete_reused_names_after_context_replacement() {
    let fixture = Fixture::new();
    let mut future = fixture.export();
    let waker = Waker::from(Arc::new(WakeCount::default()));
    assert!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    fixture.renderer.set_asset_context_active(false);
    fixture.state.texture_readback_stages.borrow_mut().clear(); // destroyed old context
    fixture.state.texture_readback_begins.set(0); // replacement driver may reuse name1
    fixture.renderer.set_asset_context_active(true);
    let mut replacement = TestDevice(fixture.state.clone());
    let fresh = replacement.begin_texture_readback(&(), 3, 2).unwrap();
    drop(future);
    assert_eq!(fixture.state.texture_readback_deletes.get(), 0);
    assert!(
        fixture
            .state
            .texture_readback_stages
            .borrow()
            .contains_key(&fresh)
    );
    replacement.delete_texture_readback(fresh);
}

#[test]
fn pressure_before_poll_and_unsupported_combinations_fail_without_gpu_work() {
    let fixture = Fixture::new();
    let mut future = fixture.export();
    fixture.account.cancelled.set(true);
    let waker = Waker::from(Arc::new(WakeCount::default()));
    assert!(matches!(
        future.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Err(_))
    ));
    assert_eq!(fixture.state.texture_readback_begins.get(), 0);
    assert!(
        fixture
            .renderer
            .asset_export_formats(ipp_core::MESH_TYPE)
            .is_empty()
    );
    assert!(
        fixture
            .renderer
            .export_asset(
                fixture.host.asset_resources().get(fixture.key).unwrap(),
                AssetExportFormat::MeshV3,
                fixture.account.clone()
            )
            .is_err()
    );
}
