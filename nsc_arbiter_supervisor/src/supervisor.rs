//! Sharded arbiter supervisor.
//!
//! This crate is the outside-world facing orchestration layer around `nsc_arbiter_core`:
//! - owns per-intent `ArbiterState`
//! - groups evidence by `intent_id`
//! - applies optional `SourceProfiles`
//! - runs the core decision functions
//!
//! No IO. No async. Concurrency is achieved by sharding state by `intent_id`.

use std::collections::{HashMap, HashSet};

use nsc_arbiter_core::{
    apply_source_profiles, arbiter_idle_tick, freeze_flags, ArbiterCfg, ArbiterEvidenceView,
    ArbiterState, Escalation, FreezeFlags, SourceProfiles,
};

use crate::adapter::{build_evidence_batch, EvidenceBuilder, SignalEvent};

/// Output action from the supervisor.
#[derive(Clone, Debug)]
pub struct ActionEvent {
    pub intent_id: String,
    pub escalation: Escalation,
    /// Optional telemetry; useful for logging/monitoring without re-aggregating.
    pub uncertainty: Option<nsc_arbiter_core::Uncertainty>,
    /// Optional freeze flags derived from text payloads.
    pub freeze_flags: Option<FreezeFlags>,
}

/// Snapshot of supervisor state for storage-agnostic persistence.
///
/// This is intentionally pure data: callers decide how/where to store it.
//#[derive(Clone, Debug, Default)]
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SupervisorSnapshot {
    /// Per-intent arbiter state.
    pub states: Vec<(String, ArbiterState)>,
}

/// Simple observability counters returned by restore/import operations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RestoreStats {
    /// Number of intent states applied from the snapshot/iterator.
    pub applied: usize,
    /// Number of existing intent states that were overwritten.
    pub overwritten: usize,
}

#[derive(Default, Debug)]
struct Shard {
    states: HashMap<String, ArbiterState>,
}

/// Deterministic FNV-1a hash (stable across runs).
fn fnv1a_u64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn shard_index(intent_id: &str, shard_count: usize) -> usize {
    if shard_count <= 1 {
        return 0;
    }
    (fnv1a_u64(intent_id) as usize) % shard_count
}

/// A sharded supervisor. One "arbiter instance" is one `(intent_id -> ArbiterState)` entry.
///
/// - `shards == 1` is the default and behaves like a single-threaded supervisor.
/// - Increasing `shards` improves throughput by reducing contention (when you later add
///   threaded execution), while keeping state isolated per shard.
#[derive(Debug)]
pub struct ArbiterSupervisor {
    cfg: ArbiterCfg,
    /// Optional per-intent cfg overrides.
    cfg_overrides: HashMap<String, ArbiterCfg>,
    profiles: Option<SourceProfiles>,
    shards: usize,
    // NOTE: State is behind a Mutex for interior mutability. This crate does not spawn threads.
    // If a caller wants to share the supervisor across threads, they can wrap the whole
    // `ArbiterSupervisor` in an `Arc` externally.
    state_shards: Vec<std::sync::Mutex<Shard>>,
}

impl ArbiterSupervisor {
    /// Create a supervisor with `shards` (concurrency count). `shards=1` is the default.
    pub fn new(shards: usize, cfg: ArbiterCfg) -> Self {
        let shards = shards.max(1);
        let mut state_shards = Vec::with_capacity(shards);
        for _ in 0..shards {
            state_shards.push(std::sync::Mutex::new(Shard::default()));
        }

        Self {
            cfg,
            cfg_overrides: HashMap::new(),
            profiles: None,
            shards,
            state_shards,
        }
    }

    /// Set source profiles. These weight evidence by `source_id`.
    pub fn set_source_profiles(&mut self, profiles: SourceProfiles) {
        self.profiles = Some(profiles);
    }

    /// Clear source profiles.
    pub fn clear_source_profiles(&mut self) {
        self.profiles = None;
    }

    /// Override cfg for a specific `intent_id`.
    pub fn set_cfg_override(&mut self, intent_id: impl Into<String>, cfg: ArbiterCfg) {
        self.cfg_overrides.insert(intent_id.into(), cfg);
    }

    /// Remove cfg override for a specific `intent_id`.
    pub fn clear_cfg_override(&mut self, intent_id: &str) {
        self.cfg_overrides.remove(intent_id);
    }

    /// Export all `(intent_id, ArbiterState)` pairs as a plain snapshot.
    ///
    /// No IO, no policy: callers decide how/where to persist this.
    /// Deterministic ordering: states are returned sorted by `intent_id`.
    pub fn snapshot(&self) -> SupervisorSnapshot {
        self.export_state()
    }

    /// Export a snapshot filtered by a caller-provided predicate.
    ///
    /// This allows FABRIC (or any caller) to persist only "active" intents.
    ///
    /// Deterministic ordering: states are returned sorted by `intent_id`.
    pub fn snapshot_filtered<F>(&self, mut predicate: F) -> SupervisorSnapshot
    where
        F: FnMut(&str, &ArbiterState) -> bool,
    {
        let mut out: Vec<(String, ArbiterState)> = Vec::new();

        // Lock shards in a stable order.
        for shard in &self.state_shards {
            let guard = shard
                .lock()
                .expect("arbiter supervisor shard mutex poisoned");
            for (k, v) in guard.states.iter() {
                if predicate(k.as_str(), v) {
                    out.push((k.clone(), v.clone()));
                }
            }
        }

        out.sort_by(|a, b| a.0.cmp(&b.0));
        SupervisorSnapshot { states: out }
    }

    /// Export a snapshot containing only the provided `intent_id`s.
    ///
    /// Deterministic ordering: states are returned sorted by `intent_id`.
    pub fn snapshot_intents(&self, intent_ids: &[&str]) -> SupervisorSnapshot {
        let want: HashSet<&str> = intent_ids.iter().copied().collect();
        self.snapshot_filtered(|id, _state| want.contains(id))
    }

    /// Export a snapshot containing only the provided owned `intent_id`s.
    ///
    /// This avoids `&str` plumbing for callers that already hold `Vec<String>`.
    /// Deterministic ordering: states are returned sorted by `intent_id`.
    pub fn snapshot_intents_owned(&self, intent_ids: &[String]) -> SupervisorSnapshot {
        let want: HashSet<&str> = intent_ids.iter().map(|s| s.as_str()).collect();
        self.snapshot_filtered(|id, _state| want.contains(id))
    }

    /// Restore supervisor state from a previously exported snapshot.
    ///
    /// This overwrites any existing per-intent state currently held by the supervisor.
    /// No IO, no policy: callers decide how the snapshot is stored.
    pub fn restore(&self, snap: SupervisorSnapshot) -> RestoreStats {
        self.import_state(snap.states)
    }

    /// Restore supervisor state by merging a snapshot into the current state.
    ///
    /// Unlike `restore()`, this does **not** clear existing state first.
    /// Snapshot entries overwrite existing entries with the same `intent_id`.
    ///
    /// This is useful when you want best-effort recovery but also want to keep any
    /// progress accumulated in-memory since the last successful save.
    pub fn restore_merge(&self, snap: SupervisorSnapshot) -> RestoreStats {
        self.import_state_merge(snap.states)
    }

    /// Export all `(intent_id, ArbiterState)` pairs.
    ///
    /// Deterministic ordering: returned vector is sorted by `intent_id`.
    pub fn export_state(&self) -> SupervisorSnapshot {
        let mut out: Vec<(String, ArbiterState)> = Vec::new();

        // Lock shards in a stable order.
        for shard in &self.state_shards {
            let guard = shard
                .lock()
                .expect("arbiter supervisor shard mutex poisoned");
            // NOTE: requires `ArbiterState: Clone` (expected for storage-agnostic snapshots).
            for (k, v) in guard.states.iter() {
                out.push((k.clone(), v.clone()));
            }
        }

        out.sort_by(|a, b| a.0.cmp(&b.0));
        SupervisorSnapshot { states: out }
    }

    /// Import `(intent_id, ArbiterState)` pairs, overwriting any existing per-intent state.
    ///
    /// The caller controls the source iterator (JSON, sqlite, LMDB, etc.).
    pub fn import_state<I>(&self, iter: I) -> RestoreStats
    where
        I: IntoIterator<Item = (String, ArbiterState)>,
    {
        // 1) Clear all current shard maps.
        for shard in &self.state_shards {
            let mut guard = shard
                .lock()
                .expect("arbiter supervisor shard mutex poisoned");
            guard.states.clear();
        }

        // 2) Re-insert into the current shard layout.
        let mut stats = RestoreStats::default();
        for (intent_id, state) in iter {
            let idx = shard_index(&intent_id, self.shards);
            let mut guard = self.state_shards[idx]
                .lock()
                .expect("arbiter supervisor shard mutex poisoned");

            // After a clear, `insert` should never overwrite, but keep the accounting correct.
            if guard.states.insert(intent_id, state).is_some() {
                stats.overwritten += 1;
            }
            stats.applied += 1;
        }

        stats
    }

    /// Import `(intent_id, ArbiterState)` pairs without clearing existing state.
    ///
    /// Snapshot entries overwrite existing entries with the same `intent_id`.
    pub fn import_state_merge<I>(&self, iter: I) -> RestoreStats
    where
        I: IntoIterator<Item = (String, ArbiterState)>,
    {
        let mut stats = RestoreStats::default();
        for (intent_id, state) in iter {
            let idx = shard_index(&intent_id, self.shards);
            let mut guard = self.state_shards[idx]
                .lock()
                .expect("arbiter supervisor shard mutex poisoned");

            if guard.states.insert(intent_id, state).is_some() {
                stats.overwritten += 1;
            }
            stats.applied += 1;
        }
        stats
    }

    /// Clear a single intent's state (useful for ops / debugging).
    pub fn clear_intent(&self, intent_id: &str) {
        let idx = shard_index(intent_id, self.shards);
        let mut guard = self.state_shards[idx]
            .lock()
            .expect("arbiter supervisor shard mutex poisoned");
        guard.states.remove(intent_id);
    }

    fn cfg_for(&self, intent_id: &str) -> &ArbiterCfg {
        self.cfg_overrides.get(intent_id).unwrap_or(&self.cfg)
    }

    fn state_for_mut(&self, intent_id: &str) -> std::sync::MutexGuard<'_, Shard> {
        let idx = shard_index(intent_id, self.shards);
        self.state_shards[idx]
            .lock()
            .expect("arbiter supervisor shard mutex poisoned")
    }

    /// Ingest a batch of outside-world events and return escalation actions.
    ///
    /// This is deterministic for a given input ordering + shard count.
    pub fn ingest<B: EvidenceBuilder>(&self, builder: &B, events: &[SignalEvent<'_>]) -> Vec<ActionEvent> {
        // 1) Build evidence records.
        let evidence = build_evidence_batch(builder, events);

        // 2) Group into per-intent views.
        let mut views: HashMap<String, ArbiterEvidenceView> = HashMap::new();
        for ev in evidence {
            views
                .entry(ev.intent_id.clone())
                .or_insert_with(|| ArbiterEvidenceView::new(ev.intent_id.clone()))
                .push(ev);
        }

        // 3) Compute optional freeze flags per intent from text payloads.
        //    We OR flags across all text entries for that intent.
        let mut ff_by_intent: HashMap<String, FreezeFlags> = HashMap::new();
        for se in events {
            if let Some(t) = &se.text {
                let ff = freeze_flags(t);
                let e = ff_by_intent.entry(se.intent_id.to_string()).or_default();
                e.rep_3p |= ff.rep_3p;
                e.stall |= ff.stall;
                e.ai_tell |= ff.ai_tell;
            }
        }

        // 4) Apply source profiles (weights) if present.
        if let Some(p) = &self.profiles {
            for view in views.values_mut() {
                apply_source_profiles(view, p);
            }
        }

        // 5) Group intents by shard to avoid lock-per-intent.
        // Determinism: we sort intent ids within each shard and also sort final outputs by intent_id.
        let mut shard_intents: Vec<Vec<String>> = vec![Vec::new(); self.shards];
        for intent_id in views.keys() {
            let idx = shard_index(intent_id, self.shards);
            shard_intents[idx].push(intent_id.clone());
        }
        for v in &mut shard_intents {
            v.sort();
        }

        // 6) Decide per shard (lock each shard once).
        let mut out: Vec<ActionEvent> = Vec::with_capacity(views.len());
        for (shard_idx, intents) in shard_intents.into_iter().enumerate() {
            if intents.is_empty() {
                continue;
            }

            let mut guard = self.state_shards[shard_idx]
                .lock()
                .expect("arbiter supervisor shard mutex poisoned");

            for intent_id in intents {
                let view = views.remove(&intent_id).expect("view existed");
                let ff = ff_by_intent.get(&intent_id).copied();

                let state = guard.states.entry(intent_id.clone()).or_default();

                // If we have freeze flags, bump hysteresis first.
                if let Some(flags) = ff {
                    state.bump(flags, self.cfg_for(&intent_id).hyst_disable);
                }

                // Core decision.
                let esc = arbiter_idle_tick(&view, ff, self.cfg_for(&intent_id), state);

                // Telemetry is optional; compute once.
                let u = Some(view.to_uncertainty());

                out.push(ActionEvent {
                    intent_id,
                    escalation: esc,
                    uncertainty: u,
                    freeze_flags: ff,
                });
            }
        }

        // Preserve the original API behavior: return actions sorted by intent_id.
        out.sort_by(|a, b| a.intent_id.cmp(&b.intent_id));
        out
    }
}
