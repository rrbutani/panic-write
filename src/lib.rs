#![no_std]

use core::fmt::Write;
use core::marker::PhantomPinned;
use core::mem::{transmute, MaybeUninit};
use core::panic::PanicInfo;
use core::pin::Pin;
use core::ptr::null_mut;

static mut PANIC_HANDLER_GETTER: Option<unsafe fn(handler: *mut (), info: &PanicInfo)> = None;
static mut PANIC_HANDLER: *mut () = null_mut();

/// Use monomorphization to "save" the type parameter of the static pointer
unsafe fn trampoline<W: Write, F: FnMut(&mut W, &PanicInfo)>(ptr: *mut (), info: &PanicInfo) {
    let handler: &mut PanicHandler<W, F> = transmute(ptr);

    // safe because self.writer is only uninit during drop
    let writer: &mut W = { &mut *handler.writer.as_mut_ptr() };

    (handler.hook)(writer, info)
}

pub struct PanicHandler<W: Write, F: FnMut(&mut W, &PanicInfo)> {
    writer: MaybeUninit<W>,
    hook: F,
    _pin: PhantomPinned,
}

fn default_hook<W: Write>(out: &mut W, info: &PanicInfo) {
    let _ = write!(out, "{}", info);
}

impl<W: Write, F: FnMut(&mut W, &PanicInfo)> PanicHandler<W, F> {
    /// Create a panic handler from a `core::fmt::Write`
    ///
    /// Note that the returned handler is detached when it goes out of scope so in most cases it's
    /// desired to keep the handler in scope for the full duration of the program.
    ///
    /// Additionally, the panic handler implements `Deref` for the provided `Write` and can be used
    /// in place of the original `Write` throughout the app.
    #[must_use = "the panic handler must be kept in scope"]
    pub fn new_with_hook(writer: W, hook: F) -> Self {
        PanicHandler {
            writer: MaybeUninit::new(writer),
            hook,
            _pin: PhantomPinned,
        }
    }

    pub fn new(writer: W) -> PanicHandler<W, fn(&mut W, &PanicInfo)> {
        // Default Hook:
        PanicHandler::<W, _>::new_with_hook(writer, default_hook::<W>)
    }

    pub fn register(self: &mut Pin<&mut Self>) {
        unsafe {
            PANIC_HANDLER_GETTER = Some(trampoline::<W, F>);
            PANIC_HANDLER = transmute(self.as_ref());
        }
    }

    /// Detach this panic handler and return the underlying writer
    pub fn detach(handler: Pin<&mut Self>) -> W {
        unsafe {
            PANIC_HANDLER_GETTER = None;
            PANIC_HANDLER = null_mut();

            // unpin is safe because the pointer to the handler is removed
            let handler = Pin::into_inner_unchecked(handler);
            let writer = core::mem::replace(&mut handler.writer, MaybeUninit::uninit());

            // safe because self.writer is only uninit during drop
            writer.assume_init()
        }
    }

    pub fn get_inner(self: Pin<&mut Self>) -> &mut W {
        unsafe { self.get_unchecked_mut() }
    }
}

// TODO: what happens we if have multiple of these?
impl<W: Write, F: FnMut(&mut W, &PanicInfo)> Drop for Pin<&mut PanicHandler<W, F>> {
    fn drop(&mut self) {
        unsafe {
            PANIC_HANDLER_GETTER = None;
            PANIC_HANDLER = null_mut();
        }
    }
}

impl<W: Write, F: FnMut(&mut W, &PanicInfo)> core::ops::Deref for PanicHandler<W, F> {
    type Target = W;

    fn deref(&self) -> &Self::Target {
        // safe because self.writer is only uninit during drop
        unsafe { &*self.writer.as_ptr() }
    }
}

impl<W: Write, F: FnMut(&mut W, &PanicInfo)> core::ops::DerefMut for PanicHandler<W, F> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // safe because self.writer is only uninit during drop
        unsafe { &mut *self.writer.as_mut_ptr() }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe {
        if let Some(trampoline) = PANIC_HANDLER_GETTER {
            trampoline(PANIC_HANDLER, info);
        }
    }
    loop {}
}
