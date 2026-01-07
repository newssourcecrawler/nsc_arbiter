#[derive(Clone, Debug)]
pub struct ArbiterCfg {
    pub tau_e: f32,
    pub tau_s: f32,
    pub tau_rep: u32,
    pub tau_stall: u32,
    pub tau_gate: f32,
    pub hyst_disable: bool,
    pub forced_rule_hits: Option<u32>,
}

impl Default for ArbiterCfg {
    fn default() -> Self {
        Self {
            tau_e: 2.2,
            tau_s: 0.76,
            tau_rep: 1,
            tau_stall: 1,
            tau_gate: 2.0,
            hyst_disable: false,
            forced_rule_hits: None,
        }
    }
}
