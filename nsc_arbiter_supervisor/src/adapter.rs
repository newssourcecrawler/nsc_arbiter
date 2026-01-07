

//! Domain adapter layer: convert outside-world signals into `nsc_arbiter_core::Evidence`.
//!
//! This module is intentionally small and policy-light:
//! - No IO
//! - No async
//! - No domain-specific rules
//!
//! Products provide an `EvidenceBuilder` (or use the provided `BasicEvidenceBuilder`) to
//! map raw `SignalEvent`s into arbiter evidence.

use std::borrow::Cow;
use std::collections::HashMap;

use nsc_arbiter_core::Evidence;

/// A raw event from the outside world (queues, sensors, finance, etc.).
///
/// The supervisor does not interpret these fields; it delegates to an `EvidenceBuilder`.
#[derive(Clone, Debug)]
pub struct SignalEvent<'a> {
    /// The supervised entity key (queue name, sensor id, portfolio bucket, etc.).
    pub intent_id: Cow<'a, str>,
    /// Originating source key (probe/feed/sensor).
    pub source_id: Cow<'a, str>,
    /// Subsystem or origin tag ("kafka", "imu", "risk", etc.).
    pub origin: Cow<'a, str>,

    /// Optional text payload (logs, status strings). If your product wants freeze-flags,
    /// it can evaluate them outside this module.
    pub text: Option<Cow<'a, str>>,

    /// Domain-provided numeric scalars (raw, not necessarily normalized).
    /// Common keys: "entropy", "cosine", "gate_shift", "weight".
    pub scalars: HashMap<Cow<'a, str>, f32>,

    /// Domain-provided guardrail trips.
    pub rule_hits: u32,
}

impl<'a> SignalEvent<'a> {
    /// Convenience constructor.
    pub fn new(
        intent_id: impl Into<Cow<'a, str>>,
        source_id: impl Into<Cow<'a, str>>,
        origin: impl Into<Cow<'a, str>>,
    ) -> Self {
        Self {
            intent_id: intent_id.into(),
            source_id: source_id.into(),
            origin: origin.into(),
            text: None,
            scalars: HashMap::new(),
            rule_hits: 0,
        }
    }

    /// Set a scalar value.
    pub fn with_scalar(mut self, key: impl Into<Cow<'a, str>>, value: f32) -> Self {
        self.scalars.insert(key.into(), value);
        self
    }

    /// Attach text payload.
    pub fn with_text(mut self, text: impl Into<Cow<'a, str>>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Set rule hits.
    pub fn with_rule_hits(mut self, hits: u32) -> Self {
        self.rule_hits = hits;
        self
    }
}

/// Lightweight normalization configuration.
///
/// This does not impose policy; it only provides optional clamping/scaling so different
/// domains can map raw values into comparable ranges.
#[derive(Clone, Copy, Debug)]
pub struct Normalizer {
    /// Clamp entropy/volatility to this maximum (values above are truncated).
    pub entropy_max: f32,
    /// Clamp gate_shift/deviation magnitude to this maximum.
    pub gate_shift_max: f32,
    /// Clamp cosine similarity to [-1, 1].
    pub clamp_cosine: bool,
}

impl Default for Normalizer {
    fn default() -> Self {
        Self {
            entropy_max: 10.0,
            gate_shift_max: 10.0,
            clamp_cosine: true,
        }
    }
}

impl Normalizer {
    #[inline]
    fn clamp01(x: f32, max: f32) -> f32 {
        if !x.is_finite() {
            return 0.0;
        }
        if x < 0.0 {
            0.0
        } else if x > max {
            max
        } else {
            x
        }
    }

    #[inline]
    fn clamp_cos(x: f32) -> f32 {
        if !x.is_finite() {
            return 0.0;
        }
        if x < -1.0 {
            -1.0
        } else if x > 1.0 {
            1.0
        } else {
            x
        }
    }

    /// Normalize the standard arbiter scalars.
    pub fn normalize(&self, mut entropy: f32, mut cosine: f32, mut gate_shift: f32, mut weight: f32) -> (f32, f32, f32, f32) {
        entropy = Self::clamp01(entropy, self.entropy_max);
        gate_shift = Self::clamp01(gate_shift, self.gate_shift_max);
        if self.clamp_cosine {
            cosine = Self::clamp_cos(cosine);
        }
        if !weight.is_finite() || weight <= 0.0 {
            weight = 1.0;
        }
        (entropy, cosine, gate_shift, weight)
    }
}

/// Trait: map a `SignalEvent` into one or more `Evidence` records.
///
/// Most domains will emit exactly one `Evidence` per event.
pub trait EvidenceBuilder {
    /// Convert one event into zero or more evidence records.
    fn build(&self, ev: &SignalEvent<'_>) -> Vec<Evidence>;
}

/// Basic builder that expects (possibly raw) scalars in `SignalEvent::scalars`:
/// - "entropy"     -> `avg_entropy`
/// - "cosine"      -> `cosine_sim`
/// - "gate_shift"  -> `gate_shift`
/// - "weight"      -> `weight`
///
/// Missing scalars default to 0.0 (weight defaults to 1.0).
#[derive(Clone, Debug)]
pub struct BasicEvidenceBuilder {
    pub normalizer: Normalizer,
    /// Optional scalar key overrides.
    pub keys: BasicKeys,
}

#[derive(Clone, Debug)]
pub struct BasicKeys {
    pub entropy: &'static str,
    pub cosine: &'static str,
    pub gate_shift: &'static str,
    pub weight: &'static str,
}

impl Default for BasicKeys {
    fn default() -> Self {
        Self {
            entropy: "entropy",
            cosine: "cosine",
            gate_shift: "gate_shift",
            weight: "weight",
        }
    }
}

impl Default for BasicEvidenceBuilder {
    fn default() -> Self {
        Self {
            normalizer: Normalizer::default(),
            keys: BasicKeys::default(),
        }
    }
}

impl EvidenceBuilder for BasicEvidenceBuilder {
    fn build(&self, ev: &SignalEvent<'_>) -> Vec<Evidence> {
        let entropy = *ev.scalars.get(self.keys.entropy).unwrap_or(&0.0);
        let cosine = *ev.scalars.get(self.keys.cosine).unwrap_or(&0.0);
        let gate_shift = *ev.scalars.get(self.keys.gate_shift).unwrap_or(&0.0);
        let weight = *ev.scalars.get(self.keys.weight).unwrap_or(&1.0);

        let (entropy, cosine, gate_shift, weight) = self.normalizer.normalize(entropy, cosine, gate_shift, weight);

        vec![Evidence {
            source_id: ev.source_id.to_string(),
            intent_id: ev.intent_id.to_string(),
            origin: ev.origin.to_string(),
            gate_shift,
            avg_entropy: entropy,
            cosine_sim: cosine,
            rule_hits: ev.rule_hits,
            weight,
        }]
    }
}

/// Helper: build evidence for a batch of events.
///
/// This is intentionally dumb; sharding/concurrency is handled by the supervisor.
pub fn build_evidence_batch<B: EvidenceBuilder>(builder: &B, events: &[SignalEvent<'_>]) -> Vec<Evidence> {
    let mut out = Vec::new();
    for ev in events {
        out.extend(builder.build(ev));
    }
    out
}