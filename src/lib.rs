use std::cell::{RefCell, RefMut};
use std::task::{Waker, Context, Poll};
use std::rc::{Rc};
use std::future::Future;
use std::pin::Pin;
use std::ops::{Deref, DerefMut};
use std::marker::PhantomData;

type WakerId = u32;

struct MutexState {
    wakers: Vec<(WakerId, Waker)>,
    next_waker_id: WakerId,
}

pub struct Mutex<T> {
    value: Rc<RefCell<T>>,
    state: Rc<RefCell<MutexState>>,
}

impl <T> Mutex<T> {
    pub fn new(value: T) -> Self {
        Mutex {
            value: Rc::new(RefCell::new(value)),
            state: Rc::new(RefCell::new(MutexState {
                wakers: vec![],
                next_waker_id: 0,
            }))
        }
    }

    pub fn lock(&self) -> LockFuture<T> {
        let waker_id = {
            let mut state = (*self.state).borrow_mut();
            let waker_id = state.next_waker_id;
            state.next_waker_id += 1;
            waker_id
        };
        let state = self.state.clone();
        LockFuture {
            waker_id,
            value: &self.value,
            state: self.state.clone(),
            set_wake: Box::new(move |waker_id, waker| {
                let mut state = (*state).borrow_mut();
                let index = state.wakers.iter().position(|(id, _waker)| *id == waker_id);
                if let Some(index) = index {
                    state.wakers.insert(index, (waker_id, waker));
                } else {
                    state.wakers.push((waker_id, waker));
                }
            }),
            phantom: PhantomData
        }
    }

    pub fn try_lock(&self) -> Option<MutexRef<T>> {
        if let Ok(v) = self.value.try_borrow_mut() {
            let r = MutexRef::new(v, self.state.clone());
            Some(r)
        } else {
            None
        }
    }
}

pub struct MutexRef<'a, T> {
    core: RefMut<'a, T>,
    on_drop: Box<dyn FnMut()>,
}

impl <'a, T> MutexRef<'a, T> {
    fn new(core: RefMut<'a, T>, state: Rc<RefCell<MutexState>>) -> Self {
        MutexRef {
            core,
            on_drop: Box::new(move || {
                let w = {
                    let mut state = (*state).borrow_mut();
                    state.wakers.pop()
                };

                if let Some((_waker_id, waker)) = w {
                    waker.wake();
                }
            }),
        }
    }
}

impl <'a, T> Deref for MutexRef<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl <'a, T> DerefMut for MutexRef<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl <'a, T> Drop for MutexRef<'a, T> {
    fn drop(&mut self) {
        (self.on_drop)();
    }
}

pub struct LockFuture<'a, T> {
    waker_id: WakerId,
    value: &'a Rc<RefCell<T>>,
    state: Rc<RefCell<MutexState>>,
    set_wake: Box<dyn FnMut(WakerId, Waker)>,
    phantom: PhantomData<&'a T>,
}

impl <'a, T: 'static> Future for LockFuture<'a, T> {
    type Output = MutexRef<'a, T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Ok(v) = self.value.try_borrow_mut() {
            let r = MutexRef::new(v, self.state.clone());
            Poll::Ready(r)
        } else {
            let waker_id = self.waker_id;
            (self.set_wake)(waker_id, cx.waker().clone());
            Poll::Pending
        }
    }
}