use crate::evidence::ArbiterEvidenceView;

// ---------------------------------------------------------------------
// Persona baselines & oddity computation
// ---------------------------------------------------------------------

/// Rolling baselines for a persona across a few core metrics.
///
/// These can be populated from SQLite logs or kept in memory and
/// periodically flushed. The intent is "per person, per time scale"
/// not global population norms.
#[derive(Clone, Debug, Default)]
pub struct PersonaBaselines {
    pub gate_shift_mu: f32,
    pub gate_shift_sigma: f32,

    pub entropy_mu: f32,
    pub entropy_sigma: f32,

    /// Baseline for (1.0 - cosine_sim) i.e. cosine distance.
    pub cos_dist_mu: f32,
    pub cos_dist_sigma: f32,
}

/// Parameters controlling how we convert per-metric z-scores into a
/// single oddity score in [0,1].
#[derive(Clone, Debug)]
pub struct OddityParams {
    /// z-threshold at which a metric is considered "surprising".
    pub z_thresh: f32,
    /// Weight on the fraction-of-surprising-metrics term.
    pub alpha: f32,
    /// Scale factor in the magnitude term's exponential.
    pub mag_scale: f32,
}

impl Default for OddityParams {
    fn default() -> Self {
        Self {
            z_thresh: 1.5, // ~86th percentile
            alpha: 0.66,   // emphasize "two-thirds of metrics are weird"
            mag_scale: 2.0,
        }
    }
}

/// Compute a per-intent oddity score in [0,1] for this ArbiterEvidenceView,
/// given per-person baselines and tunable parameters.
///
/// This is deliberately kept separate from Uncertainty so you can
/// decide later whether to:
///   - feed it into `Uncertainty.gate_shift`, or
///   - add a dedicated `oddity` field to Uncertainty.
pub fn compute_oddity(
    view: &ArbiterEvidenceView,
    baselines: &PersonaBaselines,
    params: &OddityParams,
) -> f32 {
    let mut surprising = 0_u32;
    let mut total = 0_u32;
    let mut sum_z2 = 0.0_f32;

    let eps = 1e-3_f32;

    for ev in &view.evidence {
        // 1) gate_shift z-score (higher = more odd)
        let z_gate = (ev.gate_shift - baselines.gate_shift_mu)
            / (baselines.gate_shift_sigma.abs().max(eps));
        total += 1;
        if z_gate.abs() >= params.z_thresh {
            surprising += 1;
        }
        sum_z2 += z_gate * z_gate;

        // 2) entropy z-score (higher entropy = more odd)
        let z_ent = (ev.avg_entropy - baselines.entropy_mu)
            / (baselines.entropy_sigma.abs().max(eps));
        total += 1;
        if z_ent >= params.z_thresh {
            surprising += 1;
        }
        sum_z2 += z_ent * z_ent;

        // 3) cosine distance: 1 - cosine_sim, so higher distance = more odd
        let cos_dist = 1.0_f32 - ev.cosine_sim;
        let z_cos = (cos_dist - baselines.cos_dist_mu)
            / (baselines.cos_dist_sigma.abs().max(eps));
        total += 1;
        if z_cos >= params.z_thresh {
            surprising += 1;
        }
        sum_z2 += z_cos * z_cos;

        // NOTE: you can extend this loop later to include more metrics
        // such as skin temperature delta, noise delta, etc., either by:
        //  - tucking them into Evidence extras, or
        //  - introducing a richer baselines struct.
    }

    if total == 0 {
        return 0.0;
    }

    let total_f = total as f32;
    let surprising_f = surprising as f32;

    // Fraction of "surprising" metrics: how many components are off.
    let oddity_fraction = (surprising_f / total_f).clamp(0.0, 1.0);

    // Magnitude term based on average squared z-score.
    let avg_z2 = sum_z2 / total_f;
    let magnitude_term = 1.0_f32 - (-avg_z2 / params.mag_scale).exp();

    let alpha = params.alpha.clamp(0.0, 1.0);
    let oddity_score = alpha * oddity_fraction + (1.0 - alpha) * magnitude_term;

    oddity_score.clamp(0.0, 1.0)
}