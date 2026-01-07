use std::collections::HashMap;
use crate::evidence::ArbiterEvidenceView;

// ---------------------------------------------------------------------
// Source profiles: control how much each evidence source can influence
// the aggregated uncertainty, without trusting vendor-supplied weights.
// ---------------------------------------------------------------------

/// Per-source profile describing how much we trust this source by default
/// and how far we let it deviate from that.
#[derive(Clone, Debug)]
pub struct SourceProfile {
    /// Default weight when the Evidence doesn't specify one or specifies 0.
    pub base_weight: f32,
    /// Minimum allowed weight for this source.
    pub min_weight: f32,
    /// Maximum allowed weight for this source.
    pub max_weight: f32,
}

impl SourceProfile {
    pub fn new(base_weight: f32, min_weight: f32, max_weight: f32) -> Self {
        Self {
            base_weight,
            min_weight,
            max_weight,
        }
    }

    /// Clamp a requested weight into this profile's [min, max] band,
    /// falling back to base_weight when requested is 0.
    pub fn clamp(&self, requested: f32) -> f32 {
        let w = if requested == 0.0 { self.base_weight } else { requested };
        w.clamp(self.min_weight, self.max_weight)
    }
}

pub type SourceProfiles = HashMap<String, SourceProfile>;

/// Apply source profiles to all Evidence items in an ArbiterEvidenceView.
/// This does not change any other fields; it only adjusts `weight`.
pub fn apply_source_profiles(view: &mut ArbiterEvidenceView, profiles: &SourceProfiles) {
    for ev in &mut view.evidence {
        if let Some(p) = profiles.get(&ev.source_id) {
            ev.weight = p.clamp(ev.weight);
        } else {
            // Unknown source: treat as a soft hint with low influence.
            // You can tune these defaults later.
            let default_profile = SourceProfile::new(0.2, 0.1, 0.4);
            ev.weight = default_profile.clamp(ev.weight);
        }
    }
}

/// Build a small default SourceProfiles map.
/// These are *relative* importance hints, not absolutes.
/// You can override per-persona or from config.
pub fn default_source_profiles() -> SourceProfiles {
    let mut m = SourceProfiles::new();

    // Primary LLM reasoning source
    m.insert(
        "llm".to_string(),
        SourceProfile::new(
            1.0,  // base_weight
            0.5,  // min_weight
            1.0,  // max_weight
        ),
    );

    // Speech / STT: important but secondary.
    m.insert(
        "stt".to_string(),
        SourceProfile::new(
            0.8,
            0.3,
            0.9,
        ),
    );

    // Health / vitals: noisy, moderate influence.
    m.insert(
        "health".to_string(),
        SourceProfile::new(
            0.7,
            0.2,
            0.9,
        ),
    );

    // Meta ONNX risk / valence model.
    m.insert(
        "meta_onnx".to_string(),
        SourceProfile::new(
            0.7,
            0.3,
            0.9,
        ),
    );

    // Media generators (video/audio) if you later treat them as sources.
    m.insert(
        "video_gen".to_string(),
        SourceProfile::new(
            0.6,
            0.2,
            0.8,
        ),
    );

    m.insert(
        "audio_gen".to_string(),
        SourceProfile::new(
            0.6,
            0.2,
            0.8,
        ),
    );

    // Generic vendor / external classifiers.
    m.insert(
        "vendor".to_string(),
        SourceProfile::new(
            0.4,
            0.1,
            0.6,
        ),
    );

    m
}
