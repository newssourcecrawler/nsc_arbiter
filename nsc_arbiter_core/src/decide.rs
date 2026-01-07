//nsc_arbiter_core/decide.rs

use serde::Serialize;
use serde::Deserialize;
use crate::{evidence::Uncertainty, evidence::ArbiterEvidenceView, freeze::FreezeFlags, cfg::ArbiterCfg, state::ArbiterState};

/// Arbiter decision: stay, run a one-shot critic, or (later) ask a second LLM.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Escalation {
    None,
    CritiquePass,
    SecondLLM,
}

pub fn decide_escalation_cfg(
    u: Uncertainty,
    cfg: &ArbiterCfg,
    state: &mut ArbiterState,
) -> Escalation {
    let hi_entropy = u.avg_entropy.is_finite() && u.avg_entropy > cfg.tau_e;
    let low_sim    = u.cosine_sim.is_finite() && u.cosine_sim < cfg.tau_s;
    let rules_bad  = cfg.forced_rule_hits.unwrap_or(u.rule_hits) > 0;
    let gate_bad   = u.gate_shift.is_finite() && u.gate_shift > cfg.tau_gate;

    if !hi_entropy && !low_sim && !rules_bad && !gate_bad {
        state.reset();
        return Escalation::None;
    }

    let rep_cnt   = if cfg.hyst_disable { 0 } else { state.hyst_rep };
    let stall_cnt = if cfg.hyst_disable { 0 } else { state.hyst_stall };

    if hi_entropy || low_sim || rules_bad || gate_bad || rep_cnt >= cfg.tau_rep || stall_cnt >= cfg.tau_stall {
        Escalation::CritiquePass
    } else {
        Escalation::None
    }
}

pub fn arbiter_idle_tick(
    view: &ArbiterEvidenceView,
    ff: Option<FreezeFlags>,
    cfg: &ArbiterCfg,
    state: &mut ArbiterState,
) -> Escalation {
    if let Some(flags) = ff {
        state.bump(flags, cfg.hyst_disable);
    }
    let u = view.to_uncertainty();
    decide_escalation_cfg(u, cfg, state)
}

/// Compatibility helper: decide escalation directly from an `ArbiterEvidenceView`.
///
/// This is intentionally stateless (fresh `ArbiterState`) and uses default
/// thresholds. If you want hysteresis across ticks, use `arbiter_idle_tick`
/// with a persistent `ArbiterState`.
pub fn decide_escalation_from_view(view: &ArbiterEvidenceView) -> Escalation {
    let u = view.to_uncertainty();
    let cfg = ArbiterCfg::default();
    let mut state = ArbiterState::default();
    decide_escalation_cfg(u, &cfg, &mut state)
}