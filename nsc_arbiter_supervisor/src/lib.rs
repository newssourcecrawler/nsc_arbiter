//! nsc_arbiter_supervisor
//!
//! Outside-world facing orchestration layer for `nsc_arbiter_core`.
//!
//! Responsibilities:
//! - own per-intent `ArbiterState`
//! - shard state by `intent_id` (deterministic)
//! - convert domain signals into `Evidence` via adapters
//! - invoke arbiter core decision logic
//!
//! Non-goals:
//! - no IO
//! - no async
//! - no policy logic (lives in core)

pub mod adapter;
pub mod supervisor;

pub use adapter::{
    SignalEvent,
    EvidenceBuilder,
    BasicEvidenceBuilder,
    Normalizer,
    BasicKeys,
    build_evidence_batch,
};

pub use supervisor::{
    ArbiterSupervisor,
    ActionEvent,
};
