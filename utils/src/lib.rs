use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::ptr;
use std::time;

pub mod elf;

#[cfg(not(target_arch = "wasm32"))]
type Instant = time::Instant;
#[cfg(not(target_arch = "wasm32"))]
fn now() -> Instant {
    time::Instant::now()
}

#[cfg(target_arch = "wasm32")]
use instant;
#[cfg(target_arch = "wasm32")]
type Instant = instant::Instant;
#[cfg(target_arch = "wasm32")]
fn now() -> Instant {
    instant::Instant::now()
}

pub fn read_bin_file(filename: &Path) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut file = File::open(filename)?;
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn write_bin_file(filename: &Path, data: &[u8]) -> io::Result<()> {
    let mut f = File::create(filename)?;
    f.write_all(data)?;

    Ok(())
}

pub struct FpsCounter {
    count: u32,
    timer: Instant,
}

const SECOND: time::Duration = time::Duration::from_secs(1);

impl Default for FpsCounter {
    fn default() -> FpsCounter {
        FpsCounter {
            count: 0,
            timer: now(),
        }
    }
}

impl FpsCounter {
    pub fn tick(&mut self) -> Option<u32> {
        self.count += 1;
        if self.timer.elapsed() >= SECOND {
            let fps = self.count;
            self.timer = now();
            self.count = 0;
            Some(fps)
        } else {
            None
        }
    }
}

#[macro_export]
macro_rules! index2d {
    ($x:expr, $y:expr, $w:expr) => {
        $w * $y + $x
    };
    ($t:ty, $x:expr, $y:expr, $w:expr) => {
        (($w as $t) * ($y as $t) + ($x as $t)) as $t
    };
}

#[allow(unused_macros)]
macro_rules! host_breakpoint {
    () => {
        #[cfg(debug_assertions)]
        unsafe {
            ::std::intrinsics::breakpoint()
        }
    };
}

pub mod audio {
    pub use ringbuf::{Consumer, Producer, RingBuffer};
    pub type SampleProducer = Producer<i16>;
    pub type SampleConsumer = Consumer<i16>;

    pub struct AudioRingBuffer {
        prod: SampleProducer,
        cons: SampleConsumer,
    }

    impl Default for AudioRingBuffer {
        fn default() -> AudioRingBuffer {
            AudioRingBuffer::new_with_capacity(2 * 4096)
        }
    }

    impl AudioRingBuffer {
        pub fn new_with_capacity(capacity: usize) -> AudioRingBuffer {
            let rb = RingBuffer::new(capacity);
            let (prod, cons) = rb.split();

            AudioRingBuffer { prod, cons }
        }

        pub fn producer(&mut self) -> &mut SampleProducer {
            &mut self.prod
        }

        pub fn consumer(&mut self) -> &mut SampleConsumer {
            &mut self.cons
        }

        pub fn split(self) -> (SampleProducer, SampleConsumer) {
            (self.prod, self.cons)
        }
    }
}

#[repr(transparent)]
#[derive(Clone)]
/// Wrapper for passing raw pointers around.
/// Breaks compiler safety guaranties, so must be used with care.
pub struct WeakPointer<T: ?Sized> {
    ptr: *mut T,
}

impl<T> WeakPointer<T> {
    pub fn new(ptr: *mut T) -> Self {
        WeakPointer { ptr }
    }
}

impl<T> Deref for WeakPointer<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &(*self.ptr) }
    }
}

impl<T> DerefMut for WeakPointer<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut (*self.ptr) }
    }
}

impl<T> Default for WeakPointer<T> {
    fn default() -> Self {
        WeakPointer {
            ptr: ptr::null_mut(),
        }
    }
}

use std::cell::UnsafeCell;
use std::rc::Rc;

/// Opt-out of runtime borrow checking of RefCell by using UnsafeCell
/// SAFETY: Up to the user to make sure the usage of the shared object is safe
#[repr(transparent)]
#[derive(Debug)]
pub struct Shared<T>(Rc<UnsafeCell<T>>);

impl<T> std::ops::Deref for Shared<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe { &(*self.0.get()) }
    }
}

impl<T> std::ops::DerefMut for Shared<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut (*self.0.get()) }
    }
}

impl<T> Clone for Shared<T> {
    #[inline]
    fn clone(&self) -> Shared<T> {
        Shared(self.0.clone())
    }
}

impl<T> Shared<T> {
    pub fn new(t: T) -> Shared<T> {
        Shared(Rc::new(UnsafeCell::new(t)))
    }
}

impl<T> Shared<T>
where
    T: Clone,
{
    pub fn clone_inner(&self) -> T {
        self.deref().clone()
    }
}

impl<T> Default for Shared<T>
where
    T: Default,
{
    fn default() -> Shared<T> {
        Shared::new(Default::default())
    }
}
