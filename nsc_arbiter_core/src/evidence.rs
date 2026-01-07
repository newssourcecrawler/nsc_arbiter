#[derive(Clone, Copy, Debug, Default)]
pub struct Uncertainty {
    pub avg_entropy: f32,
    pub cosine_sim:  f32,
    pub rule_hits:   u32,
    pub gate_shift:  f32,
}

#[derive(Clone, Debug)]
pub struct Evidence {
    pub source_id: String,
    pub intent_id: String,
    pub origin: String,
    // REMOVED: pub gates: GateSnapshot,
    pub gate_shift: f32,
    pub avg_entropy: f32,
    pub cosine_sim: f32,
    pub rule_hits: u32,
    pub weight: f32,
}

#[derive(Clone, Debug, Default)]
pub struct ArbiterEvidenceView {
    pub intent_id: String,
    pub evidence: Vec<Evidence>,
}

impl ArbiterEvidenceView {
    pub fn new(intent_id: impl Into<String>) -> Self {
        ArbiterEvidenceView {
            intent_id: intent_id.into(),
            evidence: Vec::new(),
        }
    }

    pub fn push(&mut self, ev: Evidence) {
        self.evidence.push(ev);
    }

    /// Aggregate all evidence into a single Uncertainty struct.
    /// Weighting is purely numeric and does not assume any extra semantics.
    pub fn to_uncertainty(&self) -> Uncertainty {
        if self.evidence.is_empty() {
            return Uncertainty {
                avg_entropy: 0.0,
                cosine_sim: 1.0,
                rule_hits:  0,
                gate_shift: 0.0,
            };
        }

        let mut sum_entropy    = 0.0_f32;
        let mut sum_cos_sim    = 0.0_f32;
        let mut sum_rule_hits  = 0.0_f32;
        let mut sum_gate_shift = 0.0_f32;
        let mut sum_w          = 0.0_f32;

        for ev in &self.evidence {
            let w = ev.weight.max(0.0);
            if w == 0.0 {
                continue;
            }
            sum_entropy    += ev.avg_entropy * w;
            sum_cos_sim    += ev.cosine_sim * w;
            sum_rule_hits  += (ev.rule_hits as f32) * w;
            sum_gate_shift += ev.gate_shift * w;
            sum_w          += w;
        }

        if sum_w > 0.0 {
            sum_entropy    /= sum_w;
            sum_cos_sim    /= sum_w;
            sum_rule_hits  /= sum_w;
            sum_gate_shift /= sum_w;
        }

        Uncertainty {
            avg_entropy: sum_entropy,
            cosine_sim:  sum_cos_sim,
            rule_hits:   sum_rule_hits.round() as u32,
            gate_shift:  sum_gate_shift,
        }
    }
}
