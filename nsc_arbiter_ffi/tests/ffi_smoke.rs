

//! FFI smoke tests.
//!
//! These tests call the exported `extern "C"` functions directly (as an external consumer would),
//! to validate:
//! - ABI surface compiles and links
//! - allocation/free symmetry for returned buffers
//! - snapshot/restore round-trip works

use std::ptr;

// Import the exported symbols from the crate under test.
// Note: `#[no_mangle] pub extern "C" fn ...` functions are visible to Rust callers too.
use nsc_arbiter_ffi::*;

fn s(s: &str) -> NscStr {
    NscStr {
        ptr: s.as_ptr(),
        len: s.len(),
    }
}

#[test]
fn ffi_version_and_default_cfg() {
    assert_eq!(nsc_arbiter_ffi_version(), NSC_ARBITER_FFI_VERSION);

    let cfg = nsc_arbiter_cfg_default();
    // Basic sanity: defaults should be finite.
    assert!(cfg.tau_e.is_finite());
    assert!(cfg.tau_s.is_finite());
    assert!(cfg.tau_gate.is_finite());
    // forced_rule_hits default is -1 (None)
    assert_eq!(cfg.forced_rule_hits, -1);
}

#[test]
fn ffi_ingest_and_free() {
    let cfg = nsc_arbiter_cfg_default();
    let h = nsc_arbiter_supervisor_new(1, cfg);
    assert!(!h.is_null());

    // One scalar key/value.
    let kv = NscScalarKV {
        key: s("entropy"),
        val: 2.0,
    };

    let ev = NscEvent {
        intent_id: s("intent:test"),
        source_id: s("source:probe"),
        origin: s("ffi"),
        text: NscStr {
            ptr: ptr::null(),
            len: 0,
        },
        scalars_len: 1,
        scalars_ptr: &kv as *const NscScalarKV,
        rule_hits: 0,
    };

    let arr = unsafe { nsc_arbiter_ingest(h, &ev as *const NscEvent, 1) };
    assert!(arr.actions_len >= 1);
    assert!(!arr.actions_ptr.is_null());

    // Read first action.
    let a0 = unsafe { &*arr.actions_ptr };
    assert!(a0.avg_entropy.is_finite());

    // Free returned buffers.
    unsafe { nsc_arbiter_actions_free(arr) };
    unsafe { nsc_arbiter_supervisor_free(h) };
}

#[test]
fn ffi_snapshot_restore_roundtrip() {
    let cfg = nsc_arbiter_cfg_default();
    let h = nsc_arbiter_supervisor_new(1, cfg);
    assert!(!h.is_null());

    // Drive one ingest so there is at least one intent state.
    let kv = NscScalarKV {
        key: s("entropy"),
        val: 1.0,
    };

    let ev = NscEvent {
        intent_id: s("intent:rt"),
        source_id: s("source:probe"),
        origin: s("ffi"),
        text: s("stall stall stall"), // likely sets a freeze flag depending on implementation
        scalars_len: 1,
        scalars_ptr: &kv as *const NscScalarKV,
        rule_hits: 0,
    };

    let arr = unsafe { nsc_arbiter_ingest(h, &ev as *const NscEvent, 1) };
    unsafe { nsc_arbiter_actions_free(arr) };

    // Snapshot.
    let snap = unsafe { nsc_arbiter_snapshot(h) };
    assert!(!snap.ptr.is_null());
    assert!(snap.len >= 12); // magic + version + count

    // Restore into the same handle (clear then load).
    let rc = unsafe { nsc_arbiter_restore(h, snap.ptr as *const u8, snap.len, 0) };
    assert_eq!(rc, 0);

    unsafe { nsc_arbiter_bytes_free(snap) };
    unsafe { nsc_arbiter_supervisor_free(h) };
}