#![allow(clippy::missing_safety_doc)]

use std::collections::HashMap;
use std::ptr;

use nsc_arbiter_core::ArbiterCfg;
use nsc_arbiter_supervisor::{ArbiterSupervisor, BasicEvidenceBuilder, SignalEvent};
use nsc_arbiter_supervisor::supervisor::SupervisorSnapshot;

/// FFI ABI version for nsc_arbiter_ffi.
///
/// Bump this when any `#[repr(C)]` struct layout or exported function signature changes.
pub const NSC_ARBITER_FFI_VERSION: u32 = 1;

#[no_mangle]
pub extern "C" fn nsc_arbiter_ffi_version() -> u32 {
    NSC_ARBITER_FFI_VERSION
}

// Snapshot wire format identification.
const SNAP_MAGIC: u32 = 0x3142_5241; // "ARB1" little-endian
const SNAP_VERSION: u32 = 1;

/// Opaque handle exposed over FFI.
#[repr(C)]
pub struct NscArbiterSupervisor {
    inner: ArbiterSupervisor,
    builder: BasicEvidenceBuilder,
}

/// FFI string view (UTF-8 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NscStr {
    pub ptr: *const u8,
    pub len: usize,
}

impl NscStr {
    fn as_str(&self) -> Option<&str> {
        if self.ptr.is_null() {
            return None;
        }
        let bytes = unsafe { std::slice::from_raw_parts(self.ptr, self.len) };
        std::str::from_utf8(bytes).ok()
    }
}

/// FFI input event.
/// The scalars map is provided as a flat array of key/value pairs.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NscEvent {
    pub intent_id: NscStr,
    pub source_id: NscStr,
    pub origin: NscStr,

    /// Optional text payload (may be null).
    pub text: NscStr,

    /// Number of scalar pairs.
    pub scalars_len: usize,
    /// Pointer to scalar pairs.
    pub scalars_ptr: *const NscScalarKV,

    pub rule_hits: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NscScalarKV {
    pub key: NscStr,  // e.g. "entropy", "cosine", "gate_shift", "weight"
    pub val: f32,
}

/// Escalation as a C-friendly enum.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NscEscalation {
    None = 0,
    CritiquePass = 1,
    SecondLLM = 2,
}

/// Output action.
/// Note: `intent_id` points into an internal owned string buffer held by the action array.
#[repr(C)]
pub struct NscAction {
    pub intent_id: NscStr,
    pub escalation: NscEscalation,

    /// Telemetry (always populated by current supervisor)
    pub avg_entropy: f32,
    pub cosine_sim: f32,
    pub gate_shift: f32,
    pub rule_hits: u32,

    /// Freeze flags
    pub ff_rep_3p: u8,
    pub ff_stall: u8,
    pub ff_ai_tell: u8,
}

/// Owned array returned over FFI.
#[repr(C)]
pub struct NscActionArray {
    pub actions_ptr: *mut NscAction,
    pub actions_len: usize,

    // backing storage for strings (one blob) so intent_id pointers stay valid
    pub strings_ptr: *mut u8,
    pub strings_len: usize,
}

/// Owned byte buffer (for snapshot).
#[repr(C)]
pub struct NscBytes {
    pub ptr: *mut u8,
    pub len: usize,
}

/// Restore result statistics (FFI-safe).
#[repr(C)]
pub struct NscRestoreStats {
    pub applied: u32,
    pub overwritten: u32,
    pub rc: i32,
}

/// Supervisor cfg for FFI (keep it minimal).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NscCfg {
    pub tau_e: f32,
    pub tau_s: f32,
    pub tau_rep: u32,
    pub tau_stall: u32,
    pub tau_gate: f32,
    pub hyst_disable: u8,
    pub forced_rule_hits: i32, // -1 means None
}

#[no_mangle]
pub extern "C" fn nsc_arbiter_cfg_default() -> NscCfg {
    let d = ArbiterCfg::default();
    NscCfg {
        tau_e: d.tau_e,
        tau_s: d.tau_s,
        tau_rep: d.tau_rep,
        tau_stall: d.tau_stall,
        tau_gate: d.tau_gate,
        hyst_disable: if d.hyst_disable { 1 } else { 0 },
        forced_rule_hits: d.forced_rule_hits.map(|v| v as i32).unwrap_or(-1),
    }
}

fn cfg_from_ffi(c: NscCfg) -> ArbiterCfg {
    ArbiterCfg {
        tau_e: c.tau_e,
        tau_s: c.tau_s,
        tau_rep: c.tau_rep,
        tau_stall: c.tau_stall,
        tau_gate: c.tau_gate,
        hyst_disable: c.hyst_disable != 0,
        forced_rule_hits: if c.forced_rule_hits < 0 { None } else { Some(c.forced_rule_hits as u32) },
    }
}

fn esc_to_ffi(e: nsc_arbiter_core::Escalation) -> NscEscalation {
    match e {
        nsc_arbiter_core::Escalation::None => NscEscalation::None,
        nsc_arbiter_core::Escalation::CritiquePass => NscEscalation::CritiquePass,
        nsc_arbiter_core::Escalation::SecondLLM => NscEscalation::SecondLLM,
    }
}

/// Create a new supervisor handle.
///
/// Notes:
/// - `shards` controls internal state sharding (intent_id -> shard).
/// - This library does not spawn threads. If you call into the same handle concurrently from
///   multiple threads, calls will serialize per-shard via internal mutexes.
#[no_mangle]
pub extern "C" fn nsc_arbiter_supervisor_new(shards: usize, cfg: NscCfg) -> *mut NscArbiterSupervisor {
    let sup = ArbiterSupervisor::new(shards.max(1), cfg_from_ffi(cfg));
    let handle = NscArbiterSupervisor {
        inner: sup,
        builder: BasicEvidenceBuilder::default(),
    };
    Box::into_raw(Box::new(handle))
}

#[no_mangle]
pub unsafe extern "C" fn nsc_arbiter_supervisor_free(h: *mut NscArbiterSupervisor) {
    if !h.is_null() {
        drop(Box::from_raw(h));
    }
}

/// Ingest events. Returns an owned action array (must be freed with `nsc_arbiter_actions_free`).
#[no_mangle]
pub unsafe extern "C" fn nsc_arbiter_ingest(
    h: *mut NscArbiterSupervisor,
    events_ptr: *const NscEvent,
    events_len: usize,
) -> NscActionArray {
    if h.is_null() || events_ptr.is_null() || events_len == 0 {
        return NscActionArray { actions_ptr: ptr::null_mut(), actions_len: 0, strings_ptr: ptr::null_mut(), strings_len: 0 };
    }

    let handle = &mut *h;
    let events = std::slice::from_raw_parts(events_ptr, events_len);

    // Build Rust SignalEvents
    let mut rust_events: Vec<SignalEvent<'static>> = Vec::with_capacity(events_len);

    for e in events {
        let intent_id = match e.intent_id.as_str() { Some(s) => s.to_string(), None => continue };
        let source_id = match e.source_id.as_str() { Some(s) => s.to_string(), None => continue };
        let origin = match e.origin.as_str() { Some(s) => s.to_string(), None => continue };

        let mut se = SignalEvent::new(intent_id, source_id, origin);
        se.rule_hits = e.rule_hits;

        // text
        if let Some(t) = e.text.as_str() {
            if !t.is_empty() {
                se.text = Some(t.to_string().into());
            }
        }

        // scalars
        if !e.scalars_ptr.is_null() && e.scalars_len > 0 {
            let kvs = std::slice::from_raw_parts(e.scalars_ptr, e.scalars_len);
            let mut map: HashMap<std::borrow::Cow<'static, str>, f32> = HashMap::new();
            for kv in kvs {
                if let Some(k) = kv.key.as_str() {
                    map.insert(k.to_string().into(), kv.val);
                }
            }
            se.scalars = map;
        }

        rust_events.push(se);
    }

    let actions = handle.inner.ingest(&handle.builder, &rust_events);

    // Build a single backing blob for intent_id strings
    let mut strings: Vec<u8> = Vec::new();
    let mut out: Vec<NscAction> = Vec::with_capacity(actions.len());
    let mut offsets: Vec<(usize, usize)> = Vec::with_capacity(actions.len());

    for a in actions {
        let start = strings.len();
        strings.extend_from_slice(a.intent_id.as_bytes());
        let len = strings.len() - start;
        offsets.push((start, len));

        let (ff_rep_3p, ff_stall, ff_ai_tell) = if let Some(ff) = a.freeze_flags {
            (ff.rep_3p as u8, ff.stall as u8, ff.ai_tell as u8)
        } else {
            (0, 0, 0)
        };

        let u = a.uncertainty.unwrap_or_default();

        out.push(NscAction {
            // fixed up after we pin the backing string blob
            intent_id: NscStr { ptr: ptr::null(), len },
            escalation: esc_to_ffi(a.escalation),
            avg_entropy: u.avg_entropy,
            cosine_sim: u.cosine_sim,
            gate_shift: u.gate_shift,
            rule_hits: u.rule_hits,
            ff_rep_3p,
            ff_stall,
            ff_ai_tell,
        });
    }

    // Pin buffers and fix pointers
    let mut strings_box = strings.into_boxed_slice();
    let strings_ptr = strings_box.as_mut_ptr();
    let strings_len = strings_box.len();

    let mut out_box = out.into_boxed_slice();
    let actions_ptr = out_box.as_mut_ptr();
    let actions_len = out_box.len();

    for (act, (off, _len)) in out_box.iter_mut().zip(offsets.into_iter()) {
        act.intent_id.ptr = strings_ptr.add(off);
    }

    // Leak boxes to caller; freed by nsc_arbiter_actions_free
    std::mem::forget(strings_box);
    std::mem::forget(out_box);

    NscActionArray {
        actions_ptr,
        actions_len,
        strings_ptr,
        strings_len,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nsc_arbiter_actions_free(arr: NscActionArray) {
    if !arr.actions_ptr.is_null() {
        let slice_ptr = std::ptr::slice_from_raw_parts_mut(arr.actions_ptr, arr.actions_len);
        drop(Box::from_raw(slice_ptr));
    }
    if !arr.strings_ptr.is_null() {
        let slice_ptr = std::ptr::slice_from_raw_parts_mut(arr.strings_ptr, arr.strings_len);
        drop(Box::from_raw(slice_ptr));
    }
}

/// Snapshot format (binary):
/// [u32 magic = "ARB1"][u32 version = 1][u32 count]
/// repeated count times:
///   [u32 strlen][bytes...][u32 hyst_rep][u32 hyst_stall]
#[no_mangle]
pub unsafe extern "C" fn nsc_arbiter_snapshot(h: *mut NscArbiterSupervisor) -> NscBytes {
    if h.is_null() {
        return NscBytes { ptr: ptr::null_mut(), len: 0 };
    }
    let handle = &mut *h;
    let snap = handle.inner.snapshot();

    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&SNAP_MAGIC.to_le_bytes());
    buf.extend_from_slice(&SNAP_VERSION.to_le_bytes());
    buf.extend_from_slice(&(snap.states.len() as u32).to_le_bytes());

    for (id, st) in snap.states {
        let idb = id.as_bytes();
        buf.extend_from_slice(&(idb.len() as u32).to_le_bytes());
        buf.extend_from_slice(idb);

        // This assumes ArbiterState has these fields (per your extracted core/supervisor usage).
        // If your ArbiterState differs, adjust here to match its actual shape.
        buf.extend_from_slice(&(st.hyst_rep as u32).to_le_bytes());
        buf.extend_from_slice(&(st.hyst_stall as u32).to_le_bytes());
    }

    let mut boxed = buf.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    let len = boxed.len();
    std::mem::forget(boxed);

    NscBytes { ptr, len }
}

#[no_mangle]
pub unsafe extern "C" fn nsc_arbiter_bytes_free(b: NscBytes) {
    if !b.ptr.is_null() {
        let slice_ptr = std::ptr::slice_from_raw_parts_mut(b.ptr, b.len);
        drop(Box::from_raw(slice_ptr));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nsc_arbiter_restore(h: *mut NscArbiterSupervisor, bytes: *const u8, len: usize, merge: u8) -> i32 {
    if h.is_null() || bytes.is_null() || len < 12 {
        return -1;
    }
    let handle = &mut *h;
    let data = std::slice::from_raw_parts(bytes, len);

    let mut i = 0usize;
    let read_u32 = |data: &[u8], i: &mut usize| -> Option<u32> {
        if *i + 4 > data.len() { return None; }
        let v = u32::from_le_bytes(data[*i..*i+4].try_into().ok()?);
        *i += 4;
        Some(v)
    };

    let magic = match read_u32(data, &mut i) { Some(v) => v, None => return -2 };
    if magic != SNAP_MAGIC {
        return -8; // bad magic
    }
    let ver = match read_u32(data, &mut i) { Some(v) => v, None => return -2 };
    if ver != SNAP_VERSION {
        return -9; // unsupported version
    }

    let count = match read_u32(data, &mut i) { Some(v) => v as usize, None => return -2 };

    let mut states: Vec<(String, nsc_arbiter_core::ArbiterState)> = Vec::with_capacity(count);

    for _ in 0..count {
        let slen = match read_u32(data, &mut i) { Some(v) => v as usize, None => return -3 };
        if i + slen > data.len() { return -4; }
        let id = match std::str::from_utf8(&data[i..i+slen]) {
            Ok(s) => s.to_string(),
            Err(_) => return -5,
        };
        i += slen;

        let hyst_rep = match read_u32(data, &mut i) { Some(v) => v, None => return -6 };
        let hyst_stall = match read_u32(data, &mut i) { Some(v) => v, None => return -7 };

        let mut st = nsc_arbiter_core::ArbiterState::default();
        st.hyst_rep = hyst_rep;
        st.hyst_stall = hyst_stall;

        states.push((id, st));
    }

    let snap = SupervisorSnapshot { states };
    if merge != 0 {
        let _stats = handle.inner.restore_merge(snap);
    } else {
        let _stats = handle.inner.restore(snap);
    }

    0
}

#[no_mangle]
pub unsafe extern "C" fn nsc_arbiter_restore_stats(
    h: *mut NscArbiterSupervisor,
    bytes: *const u8,
    len: usize,
    merge: u8,
) -> NscRestoreStats {
    if h.is_null() || bytes.is_null() || len < 12 {
        return NscRestoreStats { applied: 0, overwritten: 0, rc: -1 };
    }
    let handle = &mut *h;
    let data = std::slice::from_raw_parts(bytes, len);

    let mut i = 0usize;
    let read_u32 = |data: &[u8], i: &mut usize| -> Option<u32> {
        if *i + 4 > data.len() { return None; }
        let v = u32::from_le_bytes(data[*i..*i+4].try_into().ok()?);
        *i += 4;
        Some(v)
    };

    let magic = match read_u32(data, &mut i) { Some(v) => v, None => return NscRestoreStats { applied: 0, overwritten: 0, rc: -2 } };
    if magic != SNAP_MAGIC {
        return NscRestoreStats { applied: 0, overwritten: 0, rc: -8 };
    }
    let ver = match read_u32(data, &mut i) { Some(v) => v, None => return NscRestoreStats { applied: 0, overwritten: 0, rc: -2 } };
    if ver != SNAP_VERSION {
        return NscRestoreStats { applied: 0, overwritten: 0, rc: -9 };
    }

    let count = match read_u32(data, &mut i) { Some(v) => v as usize, None => return NscRestoreStats { applied: 0, overwritten: 0, rc: -2 } };

    let mut states: Vec<(String, nsc_arbiter_core::ArbiterState)> = Vec::with_capacity(count);

    for _ in 0..count {
        let slen = match read_u32(data, &mut i) { Some(v) => v as usize, None => return NscRestoreStats { applied: 0, overwritten: 0, rc: -3 } };
        if i + slen > data.len() {
            return NscRestoreStats { applied: 0, overwritten: 0, rc: -4 };
        }
        let id = match std::str::from_utf8(&data[i..i+slen]) {
            Ok(s) => s.to_string(),
            Err(_) => return NscRestoreStats { applied: 0, overwritten: 0, rc: -5 },
        };
        i += slen;

        let hyst_rep = match read_u32(data, &mut i) { Some(v) => v, None => return NscRestoreStats { applied: 0, overwritten: 0, rc: -6 } };
        let hyst_stall = match read_u32(data, &mut i) { Some(v) => v, None => return NscRestoreStats { applied: 0, overwritten: 0, rc: -7 } };

        let mut st = nsc_arbiter_core::ArbiterState::default();
        st.hyst_rep = hyst_rep;
        st.hyst_stall = hyst_stall;

        states.push((id, st));
    }

    let snap = SupervisorSnapshot { states };
    let stats = if merge != 0 {
        handle.inner.restore_merge(snap)
    } else {
        handle.inner.restore(snap)
    };

    NscRestoreStats {
        applied: stats.applied as u32,
        overwritten: stats.overwritten as u32,
        rc: 0,
    }
}