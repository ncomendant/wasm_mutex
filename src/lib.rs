use std::task::{Waker, Context, Poll};
use std::future::Future;
use std::pin::Pin;
use std::ops::{Deref, DerefMut};
use std::marker::PhantomData;

#[cfg(not(feature = "async"))]
type Pointer<T> = std::rc::Rc<T>;
#[cfg(feature = "async")]
type Pointer<T> = std::sync::Arc<T>;

#[cfg(not(feature = "async"))]
type MutMem<T> = std::cell::RefCell<T>;
#[cfg(feature = "async")]
type MutMem<T> = std::sync::Mutex<T>;

#[cfg(not(feature = "async"))]
type Guard<'a, T> = std::cell::RefMut<'a, T>;
#[cfg(feature = "async")]
type Guard<'a, T> = std::sync::MutexGuard<'a, T>;

type WakerId = u32;

#[derive(Debug, Clone)]
struct MutexState {
    wakers: Vec<(WakerId, Waker)>,
    next_waker_id: WakerId,
}

impl Default for MutexState {
    fn default() -> Self {
        Self {
            wakers: Default::default(),
            next_waker_id: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Mutex<T> {
    value: Pointer<MutMem<T>>,
    state: Pointer<MutMem<MutexState>>,
}

impl <T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Self { value: Default::default(), state: Default::default() }
    }
}

impl <T: serde::Serialize> serde::Serialize for Mutex<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        serializer.serialize_newtype_struct("Mutex", &*self.value)
    }
}

impl <'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Mutex<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: serde::Deserializer<'de> {
        Ok(Mutex {
            value: Pointer::new(MutMem::deserialize(deserializer)?),
            state: Default::default(),
        })
    }
}

impl <T> Mutex<T> {
    pub fn new(value: T) -> Self {
        Mutex {
            value: Pointer::new(MutMem::new(value)),
            state: Default::default(),
        }
    }

    pub fn lock(&self) -> LockFuture<T> {
        let waker_id = {
            #[cfg(not(feature = "async"))]
            let mut state = (*self.state).borrow_mut();
            #[cfg(feature = "async")]
            let mut state = (*self.state).try_lock().unwrap();
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
                #[cfg(not(feature = "async"))]
                let mut state = (*state).borrow_mut();
                #[cfg(feature = "async")]
                let mut state = (*state).try_lock().unwrap();
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
        #[cfg(not(feature = "async"))]
        let v = self.value.try_borrow_mut();
        #[cfg(feature = "async")]
        let v = self.value.try_lock();

        if let Ok(v) = v {
            let r = MutexRef::new(v, self.state.clone());
            Some(r)
        } else {
            None
        }
    }
}

pub struct MutexRef<'a, T> {
    core: Guard<'a, T>,
    on_drop: Box<dyn Fn()>,
}

impl <'a, T> MutexRef<'a, T> {
    fn new(core: Guard<'a, T>, state: Pointer<MutMem<MutexState>>) -> Self {
        MutexRef {
            core,
            on_drop: Box::new(move || {
                let w = {
                    #[cfg(not(feature = "async"))]
                    let mut state = (*state).borrow_mut();
                    #[cfg(feature = "async")]
                    let mut state = (*state).try_lock().unwrap();
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
    value: &'a Pointer<MutMem<T>>,
    state: Pointer<MutMem<MutexState>>,
    #[cfg(not(feature = "async"))]
    set_wake: Box<dyn Fn(WakerId, Waker)>,
    #[cfg(feature = "async")]
    set_wake: Box<dyn Fn(WakerId, Waker) + Send>,
    phantom: PhantomData<&'a T>,
}

impl <'a, T: 'static> Future for LockFuture<'a, T> {
    type Output = MutexRef<'a, T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        #[cfg(not(feature = "async"))]
        let v = self.value.try_borrow_mut();
        #[cfg(feature = "async")]
        let v = self.value.try_lock();

        if let Ok(v) = v {
            let r = MutexRef::new(v, self.state.clone());
            Poll::Ready(r)
        } else {
            let waker_id = self.waker_id;
            (self.set_wake)(waker_id, cx.waker().clone());
            Poll::Pending
        }
    }
}