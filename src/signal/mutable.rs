use super::Signal;
use std;
use std::fmt;
use std::pin::Pin;
use std::marker::{PhantomData, Unpin};
use std::ops::{Deref, DerefMut};
// TODO use parking_lot ?
use std::sync::{Arc, Weak, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
// TODO use parking_lot ?
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Poll, Waker, Context};

#[derive(Debug)]
pub(crate) struct ChangedWaker {
    changed: AtomicBool,
    closed: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl ChangedWaker {
    pub(crate) fn new() -> Self {
        Self {
            changed: AtomicBool::new(true),
            closed: AtomicBool::new(false),
            waker: Mutex::new(None),
        }
    }

    pub(crate) fn wake(&self, changed: bool, closed: bool) {
        let waker = {
            let mut lock = self.waker.lock().unwrap();

            if changed {
                self.changed.store(true, Ordering::SeqCst);
            }

            if closed {
                self.closed.store(true, Ordering::SeqCst);
            }
            lock.take()
        };

        if let Some(waker) = waker {
            waker.wake();
        }
    }

    pub(crate) fn set_waker(&self, cx: &Context) {
        *self.waker.lock().unwrap() = Some(cx.waker().clone());
    }

    pub(crate) fn is_changed(&self) -> bool {
        self.changed.swap(false, Ordering::SeqCst)
    }
}


#[derive(Debug)]
struct MutableLockState<A> {
    value: A,
    // TODO use HashMap or BTreeMap instead ?
    signals: Vec<Weak<ChangedWaker>>,
}

impl<A> MutableLockState<A> {
    fn push_signal(&mut self, state: &Arc<ChangedWaker>) {
        self.signals.push(Arc::downgrade(state));
    }

    fn notify(&mut self, has_changed: bool, closed: bool) {
        self.signals.retain(|signal| {
            if let Some(signal) = signal.upgrade() {
                signal.wake(has_changed, closed);
                true
            } else {
                false
            }
        });
    }
}


#[derive(Debug)]
struct MutableState<A> {
    senders: AtomicUsize,
    lock: RwLock<MutableLockState<A>>,
}


#[derive(Debug)]
struct MutableSignalState<A> {
    waker: Arc<ChangedWaker>,
    // TODO change this to Weak ?
    state: Arc<MutableState<A>>,
}

impl<A> MutableSignalState<A> {
    fn new(state: Arc<MutableState<A>>) -> Self {
        MutableSignalState {
            waker: Arc::new(ChangedWaker::new()),
            state,
        }
    }

    fn poll_change<B, F>(&self, cx: &mut Context, f: F) -> Poll<Option<B>> where F: FnOnce(&A) -> B {
        if self.waker.is_changed() {
            let value = {
                let lock = self.state.lock.read().unwrap();
                f(&lock.value)
            };
            Poll::Ready(Some(value))
        } else if self.waker.closed.load(Ordering::SeqCst) == true {
            Poll::Ready(None)
        } else {
            self.waker.set_waker(cx);
            Poll::Pending
        }
    }
}


#[derive(Debug)]
pub struct MutableLockMut<'a, A> where A: 'a {
    mutated: bool,
    lock: RwLockWriteGuard<'a, MutableLockState<A>>,
}

impl<'a, A> Deref for MutableLockMut<'a, A> {
    type Target = A;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.lock.value
    }
}

impl<'a, A> DerefMut for MutableLockMut<'a, A> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.mutated = true;
        &mut self.lock.value
    }
}

impl<'a, A> Drop for MutableLockMut<'a, A> {
    #[inline]
    fn drop(&mut self) {
        if self.mutated {
            self.lock.notify(true, false);
        }
    }
}


#[derive(Debug)]
pub struct MutableLockRef<'a, A> where A: 'a {
    lock: RwLockReadGuard<'a, MutableLockState<A>>,
}

impl<'a, A> Deref for MutableLockRef<'a, A> {
    type Target = A;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.lock.value
    }
}


#[repr(transparent)]
pub struct ReadOnlyMutable<A>(Arc<MutableState<A>>);

impl<A> ReadOnlyMutable<A> {
    // TODO return Result ?
    #[inline]
    pub fn lock_ref(&self) -> MutableLockRef<A> {
        MutableLockRef {
            lock: self.0.lock.read().unwrap(),
        }
    }

    fn push_waker(&self, waker: &Arc<ChangedWaker>) {
        if self.0.senders.load(Ordering::SeqCst) != 0 {
            self.0.lock.write().unwrap().push_signal(waker);
        }
    }

    fn signal_state(&self) -> MutableSignalState<A> {
        let signal = MutableSignalState::new(self.0.clone());

        self.push_waker(&signal.waker);

        signal
    }

    #[inline]
    pub fn signal_ref<B, F>(&self, f: F) -> MutableSignalRef<A, F> where F: FnMut(&A) -> B {
        MutableSignalRef(self.signal_state(), f)
    }
}

impl<A: Copy> ReadOnlyMutable<A> {
    #[inline]
    pub fn get(&self) -> A {
        self.0.lock.read().unwrap().value
    }

    #[inline]
    pub fn signal(&self) -> MutableSignal<A> {
        MutableSignal(self.signal_state())
    }
}

impl<A: Clone> ReadOnlyMutable<A> {
    #[inline]
    pub fn get_cloned(&self) -> A {
        self.0.lock.read().unwrap().value.clone()
    }

    #[inline]
    pub fn signal_cloned(&self) -> MutableSignalCloned<A> {
        MutableSignalCloned(self.signal_state())
    }
}

impl<A> Clone for ReadOnlyMutable<A> {
    #[inline]
    fn clone(&self) -> Self {
        ReadOnlyMutable(self.0.clone())
    }
}

impl<A> fmt::Debug for ReadOnlyMutable<A> where A: fmt::Debug {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let state = self.0.lock.read().unwrap();

        fmt.debug_tuple("ReadOnlyMutable")
            .field(&state.value)
            .finish()
    }
}

#[repr(transparent)]
pub struct Mutable<A>(ReadOnlyMutable<A>);

impl<A> Mutable<A> {
    // TODO should this inline ?
    pub fn new(value: A) -> Self {
        Self::from(value)
    }

    #[inline]
    fn state(&self) -> &Arc<MutableState<A>> {
        &(self.0).0
    }

    #[inline]
    pub fn read_only(&self) -> ReadOnlyMutable<A> {
        self.0.clone()
    }

    pub fn replace(&self, value: A) -> A {
        let mut state = self.state().lock.write().unwrap();

        let value = std::mem::replace(&mut state.value, value);

        state.notify(true, false);

        value
    }

    pub fn replace_with<F>(&self, f: F) -> A where F: FnOnce(&mut A) -> A {
        let mut state = self.state().lock.write().unwrap();

        let new_value = f(&mut state.value);
        let value = std::mem::replace(&mut state.value, new_value);

        state.notify(true, false);

        value
    }

    pub fn swap(&self, other: &Mutable<A>) {
        // TODO can this dead lock ?
        let mut state1 = self.state().lock.write().unwrap();
        let mut state2 = other.state().lock.write().unwrap();

        std::mem::swap(&mut state1.value, &mut state2.value);

        state1.notify(true, false);
        state2.notify(true, false);
    }

    pub fn set(&self, value: A) {
        let mut state = self.state().lock.write().unwrap();

        state.value = value;

        state.notify(true, false);
    }

    pub fn set_if<F>(&self, value: A, f: F) where F: FnOnce(&A, &A) -> bool {
        let mut state = self.state().lock.write().unwrap();

        if f(&state.value, &value) {
            state.value = value;
            state.notify(true, false);
        }
    }

    // TODO lots of unit tests to verify that it only notifies when the object is mutated
    // TODO return Result ?
    // TODO should this inline ?
    pub fn lock_mut(&self) -> MutableLockMut<A> {
        MutableLockMut {
            mutated: false,
            lock: self.state().lock.write().unwrap(),
        }
    }
}

impl<A> From<A> for Mutable<A> {
    #[inline]
    fn from(value: A) -> Self {
        Mutable(ReadOnlyMutable(Arc::new(MutableState {
            senders: AtomicUsize::new(1),
            lock: RwLock::new(MutableLockState {
                value: value.into(),
                signals: vec![],
            }),
        })))
    }
}

impl<A> Deref for Mutable<A> {
    type Target = ReadOnlyMutable<A>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<A: PartialEq> Mutable<A> {
    #[inline]
    pub fn set_neq(&self, value: A) {
        self.set_if(value, PartialEq::ne);
    }
}

impl<A> fmt::Debug for Mutable<A> where A: fmt::Debug {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let state = self.state().lock.read().unwrap();

        fmt.debug_tuple("Mutable")
            .field(&state.value)
            .finish()
    }
}

#[cfg(feature = "serde")]
impl<T> serde::Serialize for Mutable<T> where T: serde::Serialize {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.state().lock.read().unwrap().value.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, T> serde::Deserialize<'de> for Mutable<T> where T: serde::Deserialize<'de> {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: serde::Deserializer<'de> {
        T::deserialize(deserializer).map(Mutable::new)
    }
}

// TODO can this be derived ?
impl<T: Default> Default for Mutable<T> {
    #[inline]
    fn default() -> Self {
        Mutable::new(Default::default())
    }
}

impl<A> Clone for Mutable<A> {
    #[inline]
    fn clone(&self) -> Self {
        self.state().senders.fetch_add(1, Ordering::SeqCst);
        Mutable(self.0.clone())
    }
}

impl<A> Drop for Mutable<A> {
    #[inline]
    fn drop(&mut self) {
        let state = self.state();

        let old_senders = state.senders.fetch_sub(1, Ordering::SeqCst);

        if old_senders == 1 {
            let mut lock = state.lock.write().unwrap();

            if lock.signals.len() > 0 {
                lock.notify(false, true);
                // TODO is this necessary ?
                lock.signals = vec![];
            }
        }
    }
}


// TODO remove it from signals when it's dropped
#[derive(Debug)]
#[repr(transparent)]
#[must_use = "Signals do nothing unless polled"]
pub struct MutableSignal<A>(MutableSignalState<A>);

impl<A> Unpin for MutableSignal<A> {}

impl<A: Copy> Signal for MutableSignal<A> {
    type Item = A;

    fn poll_change(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.0.poll_change(cx, |value| *value)
    }
}


// TODO remove it from signals when it's dropped
#[derive(Debug)]
#[must_use = "Signals do nothing unless polled"]
pub struct MutableSignalRef<A, F>(MutableSignalState<A>, F);

impl<A, F> Unpin for MutableSignalRef<A, F> {}

impl<A, B, F> Signal for MutableSignalRef<A, F> where F: FnMut(&A) -> B {
    type Item = B;

    fn poll_change(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        let state = &this.0;
        let callback = &mut this.1;
        state.poll_change(cx, callback)
    }
}


// TODO it should have a single MutableSignal implementation for both Copy and Clone
// TODO remove it from signals when it's dropped
#[derive(Debug)]
#[repr(transparent)]
#[must_use = "Signals do nothing unless polled"]
pub struct MutableSignalCloned<A>(MutableSignalState<A>);

impl<A> Unpin for MutableSignalCloned<A> {}

impl<A: Clone> Signal for MutableSignalCloned<A> {
    type Item = A;

    // TODO code duplication with MutableSignal::poll
    fn poll_change(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.0.poll_change(cx, |value| value.clone())
    }
}

// this is used to hide ChangedWaker from the caller because it's private
#[derive(Debug)]
pub struct ChangedWakerWrapper<'a>(&'a Arc<ChangedWaker>);

pub trait Compute<A> {
    fn subscribe<'a>(&self, waker: &'a ChangedWakerWrapper);
    fn compute(&self) -> A;
}

#[derive(Debug)]
struct MemoLockState<A> {
    value: A,
}

#[derive(Debug)]
struct MemoState<A, COMPUTE: Compute<A>> {
    lock: RwLock<MemoLockState<A>>,
    waker: Arc<ChangedWaker>,
    compute: COMPUTE,
}

impl<A, COMPUTE: Compute<A>> MemoState<A, COMPUTE> {
    fn check_update(&self) {
        if self.waker.is_changed() {
            let mut guard = self.lock.write().unwrap();
            let new_value = self.compute.compute();
            guard.value = new_value;
        }
    }
}

#[derive(Debug)]
pub struct Memo<A, COMPUTE: Compute<A>>(Arc<MemoState<A, COMPUTE>>);

impl<A, COMPUTE: Compute<A>> Memo<A, COMPUTE> {
    pub fn new(compute: COMPUTE) -> Self {
        let initial_value = compute.compute();
        let waker = Arc::new(ChangedWaker::new());

        compute.subscribe(&ChangedWakerWrapper(&waker));

        let memo_state = MemoState {
            lock: RwLock::new(MemoLockState { value: initial_value }),
            waker,
            compute,
        };
        Self(Arc::new(memo_state))
    }

    fn signal_state(&self) -> MemoSignalState<A, COMPUTE> {
        let signal = MemoSignalState::new(self.0.clone());

        self.0.compute.subscribe(&ChangedWakerWrapper(&signal.waker));

        signal
    }
}

impl<A, COMPUTE: Compute<A>> Clone for Memo<A, COMPUTE> {
    #[inline]
    fn clone(&self) -> Self {
        Memo(self.0.clone())
    }
}

impl<A: Copy, COMPUTE: Compute<A>> Memo<A, COMPUTE> {
    pub fn get(&self) -> A {
        self.0.check_update();
        self.0.lock.read().unwrap().value
    }


    #[inline]
    pub fn signal(&self) -> MemoSignal<A, COMPUTE> {
        MemoSignal(self.signal_state())
    }
}

impl<A: Clone, COMPUTE: Compute<A>> Memo<A, COMPUTE> {
    #[inline]
    pub fn get_cloned(&self) -> A {
        self.0.check_update();
        self.0.lock.read().unwrap().value.clone()
    }

    #[inline]
    pub fn signal_cloned(&self) -> MemoSignalCloned<A, COMPUTE> {
        MemoSignalCloned(self.signal_state())
    }
}

#[derive(Debug)]
struct MemoSignalState<A, COMPUTE: Compute<A>> {
    waker: Arc<ChangedWaker>,
    // TODO change this to Weak ?
    state: Arc<MemoState<A, COMPUTE>>,
}

impl<A, COMPUTE: Compute<A>> MemoSignalState<A, COMPUTE> {
    fn new(state: Arc<MemoState<A, COMPUTE>>) -> Self {
        Self {
            waker: Arc::new(ChangedWaker::new()),
            state,
        }
    }

    fn poll_change<B, F>(&self, cx: &mut Context, f: F) -> Poll<Option<B>> where F: FnOnce(&A) -> B {
        if self.waker.is_changed() {
            self.state.check_update();
            let value = {
                let lock = self.state.lock.read().unwrap();
                f(&lock.value)
            };
            Poll::Ready(Some(value))
        } else if self.waker.closed.load(Ordering::SeqCst) == true {
            Poll::Ready(None)
        } else {
            self.waker.set_waker(cx);
            Poll::Pending
        }
    }
}

// TODO remove it from signals when it's dropped
#[derive(Debug)]
#[repr(transparent)]
#[must_use = "Signals do nothing unless polled"]
pub struct MemoSignal<A, COMPUTE: Compute<A>>(MemoSignalState<A, COMPUTE>);

impl<A, COMPUTE: Compute<A>> Unpin for MemoSignal<A, COMPUTE> {}

impl<A: Copy, COMPUTE: Compute<A>> Signal for MemoSignal<A, COMPUTE> {
    type Item = A;

    fn poll_change(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.0.poll_change(cx, |value| *value)
    }
}


// TODO remove it from signals when it's dropped
#[derive(Debug)]
#[must_use = "Signals do nothing unless polled"]
pub struct MemoSignalRef<A, COMPUTE: Compute<A>, F>(MemoSignalState<A, COMPUTE>, F);

impl<A, COMPUTE: Compute<A>, F> Unpin for MemoSignalRef<A, COMPUTE, F> {}

impl<A, COMPUTE: Compute<A>, B, F> Signal for MemoSignalRef<A, COMPUTE, F> where F: FnMut(&A) -> B {
    type Item = B;

    fn poll_change(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        let state = &this.0;
        let callback = &mut this.1;
        state.poll_change(cx, callback)
    }
}


// TODO it should have a single MutableSignal implementation for both Copy and Clone
// TODO remove it from signals when it's dropped
#[derive(Debug)]
#[repr(transparent)]
#[must_use = "Signals do nothing unless polled"]
pub struct MemoSignalCloned<A, COMPUTE: Compute<A>>(MemoSignalState<A, COMPUTE>);

impl<A, COMPUTE: Compute<A>> Unpin for MemoSignalCloned<A, COMPUTE> {}

impl<A: Clone, COMPUTE: Compute<A>> Signal for MemoSignalCloned<A, COMPUTE> {
    type Item = A;

    // TODO code duplication with MutableSignal::poll
    fn poll_change(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.0.poll_change(cx, |value| value.clone())
    }
}

pub trait Reader<A>: Clone {
    fn subscribe<'a>(&self, waker: &'a ChangedWakerWrapper);
    fn get(&self) -> A;
}

impl<A> Reader<A> for ReadOnlyMutable<A> where A: Clone {
    fn subscribe<'a>(&self, waker: &'a ChangedWakerWrapper) {
        self.push_waker(waker.0);
    }
    fn get(&self) -> A {
        ReadOnlyMutable::get_cloned(self)
    }
}

impl<A: Clone, COMPUTE: Compute<A>> Reader<A> for Memo<A, COMPUTE> {
    fn subscribe<'a>(&self, waker: &'a ChangedWakerWrapper) {
        self.0.compute.subscribe(waker);
    }
    fn get(&self) -> A {
        Memo::get_cloned(self)
    }
}

pub trait IntoReader<A: Clone> {
    type Target: Reader<A>;

    fn get_reader(&self) -> Self::Target;
}

impl<A: Clone> IntoReader<A> for Mutable<A> {
    type Target = ReadOnlyMutable<A>;

    fn get_reader(&self) -> Self::Target {
        self.read_only().clone()
    }
}

impl<A: Clone> IntoReader<A> for ReadOnlyMutable<A> {
    type Target = ReadOnlyMutable<A>;

    fn get_reader(&self) -> Self::Target {
        self.clone()
    }
}

impl<A: Clone, COMPUTE: Compute<A>> IntoReader<A> for Memo<A, COMPUTE> {
    type Target = Memo<A, COMPUTE>;

    fn get_reader(&self) -> Self::Target {
        self.clone()
    }
}


use paste::paste;

macro_rules! create_compute {
    ($compute_name:expr, $($param:expr),*) => {
        paste! {
            #[derive(Debug)]
            pub struct [<Compute$compute_name>]<R, $([<A$param>], [<I$param>]),*, F>
                where
                    $([<A$param>]: Clone, [<I$param>]: IntoReader<[<A$param>]>),*,
                    F: Fn($([<A$param>]),*) -> R {
                $([<p$param>]: [<I$param>]::Target, [<phantom$param>]: PhantomData<[<A$param>]>), *,
                f: F,
            }

            impl<R, $([<A$param>], [<I$param>]),*, F> [<Compute$compute_name>]<R, $([<A$param>], [<I$param>]),*, F>
                where
                    $([<A$param>]: Clone, [<I$param>]: IntoReader<[<A$param>]>),*,
                    F: Fn($([<A$param>]),*) -> R {
                pub fn new($([<p$param>]: &[<I$param>]),*, f: F) -> Self {
                    Self {
                        $([<p$param>]: [<p$param>].get_reader(), [<phantom$param>]: PhantomData),*,
                        f,
                    }
                }
            }

            impl<R, $([<A$param>], [<I$param>]),*, F> Compute<R> for [<Compute$compute_name>]<R, $([<A$param>], [<I$param>]),*, F>
                where
                    $([<A$param>]: Clone, [<I$param>]: IntoReader<[<A$param>]>),*,
                    F: Fn($([<A$param>]),*) -> R {
                fn subscribe<'a>(&self, waker: &'a ChangedWakerWrapper) {
                    $(self.[<p$param>].subscribe(waker));*;
                }

                fn compute(&self) -> R {
                    (self.f)($(self.[<p$param>].get()),*,)
                }
            }
        }
    }
}

create_compute!(1, 1);
create_compute!(2, 1, 2);
create_compute!(3, 1, 2, 3);
create_compute!(4, 1, 2, 3, 4);
create_compute!(5, 1, 2, 3, 4, 5);

