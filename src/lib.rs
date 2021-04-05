use std::cell::{RefCell, Ref, RefMut};
use std::task::{Waker, Context, Poll};
use std::rc::{Rc};
use std::ops::{Deref, DerefMut};
use std::future::Future;
use std::pin::Pin;
use js_wasm::{ClosureWrapper, set_timeout};

const LOCK_CHECK_FREQUENCY: u32 = 50;

pub struct BlockingRefCell<T> {
    inner: Rc<RefCell<T>>,
}

impl <T: 'static> BlockingRefCell<T> {
    pub fn new(value: T) -> Self {
        BlockingRefCell {
            inner: Rc::new(RefCell::new(value)),
        }
    }

    pub async fn lock(&self) -> BlockingRef<'_, T> {
        BorrowFuture::new(Rc::clone(&self.inner), LOCK_CHECK_FREQUENCY).await;
        BlockingRef {
            inner: (*self.inner).borrow()
        }
    }

    pub async fn lock_mut(&self) -> BlockingRefMut<'_, T> {
        BorrowMutFuture::new(Rc::clone(&self.inner), LOCK_CHECK_FREQUENCY).await;
        BlockingRefMut {
            inner: (*self.inner).borrow_mut()
        }
    }
}

pub struct BlockingRef<'a, T> {
    inner: Ref<'a, T>,
}

impl <'a, T> Deref for BlockingRef<'a, T> {
    type Target = Ref<'a, T>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

pub struct BlockingRefMut<'a, T> {
    inner: RefMut<'a, T>,
}

impl <'a, T> Deref for BlockingRefMut<'a, T> {
    type Target = RefMut<'a, T>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl <'a, T> DerefMut for BlockingRefMut<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

struct BorrowSharedState<T> {
    model: Rc<RefCell<T>>,
    waker: Option<Waker>,
    closure: Option<ClosureWrapper>,
}

struct BorrowFuture<T> {
    shared_state: Rc<RefCell<BorrowSharedState<T>>>,
}

impl <T> Future for BorrowFuture<T> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Ok(mut shared_state) = self.shared_state.try_borrow_mut() {
            if shared_state.model.try_borrow().is_ok() {
                Poll::Ready(())
            } else {
                shared_state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        } else {
            Poll::Pending
        }
    }
}

impl <T: 'static> BorrowFuture<T> {
    fn new(model: Rc<RefCell<T>>, duration: u32) -> Self {
        let shared_state = Rc::new(RefCell::new(BorrowSharedState { model, waker: None, closure: None }));
        BorrowFuture::check(shared_state.clone(), duration);
        BorrowFuture {
            shared_state
        }
    }

    fn check(state: Rc<RefCell<BorrowSharedState<T>>>, duration: u32) {
        let state_clone = state.clone();
        let mut unlocked_state = (*state_clone).borrow_mut();

        if unlocked_state.model.try_borrow().is_ok() {
            unlocked_state.closure = None;
            if let Some(waker) = unlocked_state.waker.take() {
                waker.wake();
            }
        } else {
            let c = set_timeout(duration, move || {
                BorrowFuture::check(state.clone(), duration);
            });

            unlocked_state.closure = Some(c.1);
        }
    }
}

struct BorrowMutFuture<T> {
    shared_state: Rc<RefCell<BorrowSharedState<T>>>,
}

impl <T> Future for BorrowMutFuture<T> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Ok(mut shared_state) = self.shared_state.try_borrow_mut() {
            if shared_state.model.try_borrow_mut().is_ok() {
                Poll::Ready(())
            } else {
                shared_state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        } else {
            Poll::Pending
        }
    }
}

impl <T: 'static> BorrowMutFuture<T> {
    fn new(model: Rc<RefCell<T>>, duration: u32) -> Self {
        let shared_state = Rc::new(RefCell::new(BorrowSharedState { model, waker: None, closure: None }));
        BorrowMutFuture::check(shared_state.clone(), duration);
        BorrowMutFuture {
            shared_state
        }
    }

    fn check(state: Rc<RefCell<BorrowSharedState<T>>>, duration: u32) {
        let state_clone = state.clone();
        let mut unlocked_state = (*state_clone).borrow_mut();

        if unlocked_state.model.try_borrow_mut().is_ok() {
            unlocked_state.closure = None;
            if let Some(waker) = unlocked_state.waker.take() {
                waker.wake();
            }
        } else {
            let c = set_timeout(duration, move || {
                BorrowFuture::check(state.clone(), duration);
            });

            unlocked_state.closure = Some(c.1);
        }
    }
}