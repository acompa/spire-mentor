//! The binary's global allocator: mimalloc, told never to purge, as in the
//! `sts2_rs` extension (`sts2-py/src/allocator.rs`, design items A1/A1b in
//! `docs/perf/2026-10-13-engine-speed-exact-design.md` §7): the advisor runs
//! its search seconds apart, and with mimalloc's default 1,000 ms purge delay
//! every search faulted the previous tree's pages back in.
#![allow(unsafe_code)]

use std::ffi::{c_int, c_long};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// `mi_option_purge_delay` (index 15 of mimalloc.h's `mi_option_t`; see the
/// extension's twin for why it is written out).
const MI_OPTION_PURGE_DELAY: c_int = 15;

extern "C" {
    fn mi_option_set(option: c_int, value: c_long);
}

/// Tell mimalloc never to purge freed memory. Call first thing in `main`.
pub fn never_purge() {
    // SAFETY: a plain C call with an in-range option index; mimalloc only
    // stores the value. It runs at the top of `main`, before any other
    // thread exists.
    unsafe { mi_option_set(MI_OPTION_PURGE_DELAY, -1) };
}
