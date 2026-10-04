use super::*;
use crate::services::render::asset_context::RenderAssetContext;

#[derive(Default)]
struct Account {
    reservations: RefCell<Vec<usize>>,
    cancelled: IoCancellation,
}

impl AssetOutputObserver for Account {
    fn reserve(&self, bytes: usize) -> Result<(), String> {
        self.check()?;
        self.reservations.borrow_mut().push(bytes);
        Ok(())
    }

    fn check(&self) -> Result<(), String> {
        if self.cancelled.is_cancelled() {
            Err("Output revoked".into())
        } else {
            Ok(())
        }
    }
}

#[test]
fn pending_stage_and_final_output_capacity_are_charged_together_without_losing_cancellation() {
    let account = Rc::new(Account::default());
    let context = RenderAssetContext::default();
    let observer = ExportObserver {
        observer: account.clone(),
        available: IoCancellation::default(),
        context: context.lease(),
        staging: Cell::new(8192),
        output: Cell::new(0),
    };
    observer.reserve(1024).unwrap();
    observer.reserve(2048).unwrap();
    assert_eq!(&*account.reservations.borrow(), &[9216, 10240]);
    observer.available.cancel();
    assert!(observer.reserve(4096).is_err());
    assert_eq!(observer.output.get(), 2048);
    assert_eq!(account.reservations.borrow().len(), 2);
}

#[test]
fn restored_context_never_authorizes_a_previous_staged_generation() {
    let account = Rc::new(Account::default());
    let context = RenderAssetContext::default();
    let observer = ExportObserver {
        observer: account,
        available: IoCancellation::default(),
        context: context.lease(),
        staging: Cell::new(0),
        output: Cell::new(0),
    };
    observer.check().unwrap();
    context.set_active(false);
    assert!(observer.check().is_err());
    context.set_active(true);
    assert!(observer.check().is_err());
}

#[test]
fn host_pressure_stops_before_any_staging_or_output_reservation() {
    let account = Rc::new(Account::default());
    account.cancelled.cancel();
    let observer = ExportObserver {
        observer: account.clone(),
        available: IoCancellation::default(),
        context: RenderAssetContext::default().lease(),
        staging: Cell::new(4096),
        output: Cell::new(0),
    };
    assert!(observer.reserve(65536).is_err());
    assert!(account.reservations.borrow().is_empty());
}
