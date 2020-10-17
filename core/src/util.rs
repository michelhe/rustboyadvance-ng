use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::ptr;
use std::time;

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

use crate::GameBoyAdvance;
#[cfg(feature = "gdb")]
use gdbstub;
#[cfg(feature = "gdb")]
use gdbstub::GdbStub;
use std::fmt;
#[cfg(feature = "gdb")]
use std::net::TcpListener;
use std::net::ToSocketAddrs;

pub fn spawn_and_run_gdb_server<A: ToSocketAddrs + fmt::Display>(
    #[allow(unused)] target: &mut GameBoyAdvance,
    #[allow(unused)] addr: A,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "gdb")]
    {
        info!("spawning gdbserver, listening on {}", addr);

        let sock = TcpListener::bind(addr)?;
        let (stream, addr) = sock.accept()?;

        info!("got connection from {}", addr);

        let mut gdb = GdbStub::new(stream);
        let result = match gdb.run(target) {
            Ok(state) => {
                info!("Disconnected from GDB. Target state: {:?}", state);
                Ok(())
            }
            Err(gdbstub::Error::TargetError(e)) => Err(e),
            Err(e) => return Err(e.into()),
        };

        info!("Debugger session ended, result={:?}", result);
    }
    #[cfg(not(feature = "gdb"))]
    {
        error!("failed. please compile me with 'gdb' feature")
    }

    Ok(())
}

pub fn read_bin_file(filename: &Path) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut file = File::open(filename)?;
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn write_bin_file(filename: &Path, data: &Vec<u8>) -> io::Result<()> {
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

    pub struct AudioRingBuffer {
        prod: Producer<i16>,
        cons: Consumer<i16>,
    }

    impl AudioRingBuffer {
        pub fn new() -> AudioRingBuffer {
            AudioRingBuffer::new_with_capacity(2 * 4096)
        }

        pub fn new_with_capacity(capacity: usize) -> AudioRingBuffer {
            let rb = RingBuffer::new(capacity);
            let (prod, cons) = rb.split();

            AudioRingBuffer { prod, cons }
        }

        pub fn producer(&mut self) -> &mut Producer<i16> {
            &mut self.prod
        }

        pub fn consumer(&mut self) -> &mut Consumer<i16> {
            &mut self.cons
        }

        pub fn split(self) -> (Producer<i16>, Consumer<i16>) {
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

    pub unsafe fn inner_unsafe(&self) -> &mut T {
        &mut (*self.0.get())
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
