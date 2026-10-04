//! Renderer readiness independent of Host task polling and source acquisition.

use std::{
    cell::RefCell,
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[derive(Clone)]
pub(crate) struct RenderAssetContext(Rc<RefCell<State>>);

struct State {
    active: bool,
    generation: u64,
    next_waiter: u64,
    waiters: BTreeMap<u64, Waker>,
}

impl Default for RenderAssetContext {
    fn default() -> Self {
        Self(Rc::new(RefCell::new(State {
            active: true,
            generation: 1,
            next_waiter: 0,
            waiters: BTreeMap::new(),
        })))
    }
}

impl RenderAssetContext {
    pub(crate) fn set_active(&self, active: bool) {
        let waiters = {
            let mut state = self.0.borrow_mut();
            if state.active && !active {
                state.generation = state
                    .generation
                    .checked_add(1)
                    .expect("render context identity exhausted");
            }
            state.active = active;
            if active {
                std::mem::take(&mut state.waiters)
            } else {
                BTreeMap::new()
            }
        };
        for waker in waiters.into_values() {
            waker.wake();
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.0.borrow().active
    }

    pub(crate) fn wait(&self) -> RenderContextWait {
        RenderContextWait {
            context: self.clone(),
            waiter: None,
        }
    }

    pub(crate) fn lease(&self) -> RenderAssetLease {
        RenderAssetLease {
            context: self.clone(),
            generation: self.0.borrow().generation,
        }
    }
}

pub(crate) struct RenderContextWait {
    context: RenderAssetContext,
    waiter: Option<u64>,
}

impl Future for RenderContextWait {
    type Output = RenderAssetLease;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut state = this.context.0.borrow_mut();
        if state.active {
            if let Some(waiter) = this.waiter.take() {
                state.waiters.remove(&waiter);
            }
            return Poll::Ready(RenderAssetLease {
                context: this.context.clone(),
                generation: state.generation,
            });
        }
        let waiter = match this.waiter {
            Some(waiter) => waiter,
            None => {
                state.next_waiter = state
                    .next_waiter
                    .checked_add(1)
                    .expect("render waiter identity exhausted");
                this.waiter = Some(state.next_waiter);
                state.next_waiter
            }
        };
        state.waiters.insert(waiter, cx.waker().clone());
        Poll::Pending
    }
}

impl Drop for RenderContextWait {
    fn drop(&mut self) {
        if let Some(waiter) = self.waiter {
            self.context.0.borrow_mut().waiters.remove(&waiter);
        }
    }
}

/// GPU handles may be deleted only through their live creating context.
#[derive(Clone)]
pub(crate) struct RenderAssetLease {
    context: RenderAssetContext,
    generation: u64,
}

impl RenderAssetLease {
    pub(crate) fn is_current(&self) -> bool {
        let state = self.context.0.borrow();
        state.active && state.generation == self.generation
    }
}

#[cfg(test)]
#[path = "asset_context_tests.rs"]
mod tests;
