//! E4 independent NATIVE LINUX no_std host. This file is deliberately NOT
//! compiled into aw libraries. Its only authority is a deterministic local
//! fake device and the shared embedded Agent's real provider/tool/journal
//! turn. The host supplies the allocator, panic policy and explicit raw Linux
//! write/exit syscalls; NO libc, CRT, rust std, OS filesystem or networking.
//!
//! This is an x86_64 Linux libc-free native executable test host, NOT a
//! freestanding firmware/ELF image, embedded ARM runtime, or C SDK/ABI.
//! Future true bare-metal hosts require their own target + bootloader/board.
//!
//! Safety: this independent example embeds 4MiB static BSS, uses an atomic
//! bump allocator and raw syscalls. The audited reusable aw libraries remain
//! entirely safe Rust with #![forbid(unsafe_code)].
#![no_std]
#![no_main]
#![deny(warnings)]

use core::{
    alloc::{GlobalAlloc, Layout},
    arch::{asm, global_asm},
    cell::UnsafeCell,
    ffi::c_void,
    panic::PanicInfo,
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
compile_error!("E4 Linux raw-syscall runtime is x86_64 Linux only; never label it firmware");

const CAPACITY: usize = 4 * 1024 * 1024;
const MAX_ALIGNMENT: usize = 4096;

#[repr(align(4096))]
struct Storage([u8; CAPACITY]);
struct Heap(UnsafeCell<Storage>);
// Thread safety on this specifically host-owned test heap is supplied only
// through NEXT's atomic allocation; the model/tools are driven sequentially.
unsafe impl Sync for Heap {}

static HEAP: Heap = Heap(UnsafeCell::new(Storage([0; CAPACITY])));
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Bump;
#[global_allocator]
static ALLOC: Bump = Bump;

unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() > MAX_ALIGNMENT {
            return ptr::null_mut();
        }
        let mask = layout.align() - 1;
        let result = NEXT.fetch_update(Ordering::AcqRel, Ordering::Acquire, |previous| {
            let aligned = previous.checked_add(mask).map(|n| n & !mask)?;
            aligned
                .checked_add(layout.size())
                .filter(|end| *end <= CAPACITY)
        });
        match result {
            Ok(previous) => {
                let aligned = (previous + mask) & !mask;
                // SAFETY: Bump allocation bounds checked atomically above,
                // static base aligned to 4096, and every region nonoverlap.
                unsafe { (*HEAP.0.get()).0.as_mut_ptr().add(aligned) }
            }
            Err(_) => ptr::null_mut(),
        }
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {
        // Bump's entire transient lifetime belongs to this single one-turn
        // host executable; no per-object deallocation is supported.
    }
}

/// Raw Linux x86_64 syscall write(2), NO libc shim.
#[inline]
unsafe fn linux_write(fd: usize, buf: *const u8, len: usize) -> isize {
    let result: isize;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") 1usize => result,
            in("rdi") fd,
            in("rsi") buf,
            in("rdx") len,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    result
}

/// Raw Linux x86_64 syscall exit(2), NOT libc exit().
#[inline]
fn linux_exit(status: usize) -> ! {
    unsafe {
        asm!(
            "syscall",
            in("rax") 60usize,
            in("rdi") status,
            options(noreturn),
        );
    }
}

fn write_all(bytes: &[u8]) -> bool {
    let mut remaining = bytes;
    while !remaining.is_empty() {
        // SAFETY: borrowed slice is a valid user-space buffer for this
        // synchronous syscall; no concurrent mutation of its memory.
        let written = unsafe { linux_write(1, remaining.as_ptr(), remaining.len()) };
        if written <= 0 {
            return false;
        }
        remaining = &remaining[written as usize..];
    }
    true
}

fn hex_nibble(nibble: u8) -> u8 {
    match nibble {
        0..=9 => b'0' + nibble,
        _ => b'A' + nibble - 10,
    }
}

// Host-owned compiler memory functions, no libc / CRT dependency.
#[no_mangle]
pub unsafe extern "C" fn memcpy(dst: *mut c_void, src: *const c_void, len: usize) -> *mut c_void {
    let dst = dst.cast::<u8>();
    let src = src.cast::<u8>();
    for i in 0..len {
        // SAFETY: compiler promises both ranges are valid and nonoverlapping.
        let byte = unsafe { ptr::read_volatile(src.add(i)) };
        unsafe { ptr::write_volatile(dst.add(i), byte) };
    }
    dst.cast()
}
#[no_mangle]
pub unsafe extern "C" fn memset(dst: *mut c_void, value: i32, len: usize) -> *mut c_void {
    let dst = dst.cast::<u8>();
    for i in 0..len {
        // SAFETY: compiler promises this destination range is writable.
        unsafe { ptr::write_volatile(dst.add(i), value as u8) };
    }
    dst.cast()
}
#[no_mangle]
pub unsafe extern "C" fn memcmp(left: *const c_void, right: *const c_void, len: usize) -> i32 {
    let left = left.cast::<u8>();
    let right = right.cast::<u8>();
    for i in 0..len {
        // SAFETY: compiler promises both ranges are valid and readable.
        let x = unsafe { ptr::read_volatile(left.add(i)) };
        let y = unsafe { ptr::read_volatile(right.add(i)) };
        if x != y {
            return x as i32 - y as i32;
        }
    }
    0
}
// LLVM may lower equality-only comparisons to bcmp in optimized builds.
// The no_std final host owns this ABI primitive; never link libSystem/libc.
#[no_mangle]
pub unsafe extern "C" fn bcmp(left: *const c_void, right: *const c_void, len: usize) -> i32 {
    // SAFETY: bcmp uses the same valid readable byte-range contract as memcmp.
    unsafe { memcmp(left, right, len) }
}

#[no_mangle]
pub unsafe extern "C" fn memmove(dst: *mut c_void, src: *const c_void, len: usize) -> *mut c_void {
    let dst = dst.cast::<u8>();
    let src = src.cast::<u8>();
    let backward = (dst as usize) > (src as usize) && (dst as usize) - (src as usize) < len;
    if backward {
        for i in (0..len).rev() {
            // SAFETY: reverse copy preserves contents for overlapping buffers.
            let byte = unsafe { ptr::read_volatile(src.add(i)) };
            unsafe { ptr::write_volatile(dst.add(i), byte) };
        }
    } else {
        for i in 0..len {
            let byte = unsafe { ptr::read_volatile(src.add(i)) };
            unsafe { ptr::write_volatile(dst.add(i), byte) };
        }
    }
    dst.cast()
}
// Prebuilt core/alloc Rust Linux GNU contains unreachable personality
// references even when rebuilt application deps use panic=abort. Abort
// immediately if invoked; no libunwind or CRT is linked.
#[no_mangle]
pub extern "C" fn rust_eh_personality() -> ! {
    linux_exit(126)
}
fn selftest_memory_primitives() -> bool {
    let mut buffer = [0u8; 16];
    // SAFETY: these test ranges are sized, valid stack buffers.
    unsafe {
        let _ = memset(buffer.as_mut_ptr().cast(), 0xA7, buffer.len());
        if buffer.iter().any(|v| *v != 0xA7) {
            return false;
        }
        let src = [0u8, 1, 2, 3, 4, 5, 6, 7];
        let _ = memcpy(buffer.as_mut_ptr().cast(), src.as_ptr().cast(), src.len());
        if memcmp(buffer.as_ptr().cast(), src.as_ptr().cast(), src.len()) != 0 {
            return false;
        }
        if bcmp(buffer.as_ptr().cast(), src.as_ptr().cast(), src.len()) != 0 {
            return false;
        }

        if memcmp(src.as_ptr().cast(), buffer[8..].as_ptr().cast(), 8) >= 0 {
            return false;
        }
        let _ = memmove(buffer.as_mut_ptr().add(2).cast(), buffer.as_ptr().cast(), 6);
        if buffer[..8] != [0, 1, 0, 1, 2, 3, 4, 5] {
            return false;
        }
        let _ = memmove(buffer.as_mut_ptr().cast(), buffer.as_ptr().add(2).cast(), 6);
        if buffer[..8] != [0, 1, 2, 3, 4, 5, 4, 5] {
            return false;
        }
    }
    true
}
#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    linux_exit(127)
}

// Linux enters _start with a kernel initial stack, not the ordinary
// System V function-call stack. Align %rsp and issue a proper CALL into
// Rust to satisfy the usual call-ABI alignment and frame assumptions.
global_asm!(
    ".global _start",
    ".type _start,@function",
    "_start:",
    "xor rbp, rbp",
    "and rsp, -16",
    "call {rust_entry}",
    "ud2",
    rust_entry = sym native_rust_entry,
);

extern "C" fn native_rust_entry() -> ! {
    if core::hint::black_box(cfg!(e4_test_panic)) {
        panic!("forced independent no_std host panic must terminate without unwinding");
    }
    if !selftest_memory_primitives() {
        let _ = write_all(b"E4_NATIVE_MEMORY_PRIMITIVE_FAILURE\n");
        linux_exit(5)
    }
    // A native host must NOT construct a second agent loop, cache a canned
    // answer, infer a missing provider, or assume any effect was rolled back.
    // The existing independent E3 example drives the very same hosted
    // TurnMachine, calls model twice and tool once, verifies durable-order
    // intent/receipt, permission source and final answer before return.
    let code = aw_embedded_example::run_demo();
    if code != 0xA80E0001 || cfg!(e4_reject_witness) {
        let _ = write_all(b"E4_NATIVE_AW_AGENT_FAILED\n");
        linux_exit(1)
    }

    let trace = aw_embedded_example::actual_portable_trace();
    let Some(events) = trace else {
        let _ = write_all(b"E4_NATIVE_TRACE_MISSING\n");
        linux_exit(2)
    };
    if events.len() != 10 {
        let _ = write_all(b"E4_NATIVE_TRACE_MISMATCH\n");
        linux_exit(3)
    }

    // Fixed-size zero-alloc output shows actual returned code and trace size.
    // It is derived from an *executed* shared embedded Agent, not hardcoded
    // opaque "pass" stdout irrespective of the model/tool effects.
    let mut line = *b"E4_NATIVE_AGENT_OK code=0x00000000 events=00\n";
    for offset in 0..8 {
        let shift = (7 - offset) * 4;
        line[26 + offset] = hex_nibble(((code >> shift) as u8) & 0xF);
    }
    line[42] = b'0' + (events.len() / 10) as u8;
    line[43] = b'0' + (events.len() % 10) as u8;
    if !write_all(&line) {
        linux_exit(4)
    }
    linux_exit(0)
}
