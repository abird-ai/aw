//! Native OS staticlib packaging ONLY for independently compiled C clients.
//! The underlying aw-embedded-ffi remains no_std+alloc; this wrapper owns
//! standard C-host linker/crt/panic/runtime dependencies, NOT the Agent core.
#[no_mangle]
pub extern "C" fn aw_e5_link_anchor() -> u32 {
    aw_embedded_ffi::aw_e5_abi_version()
}
