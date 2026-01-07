use nsc_arbiter_core::*;

#[test]
fn by_entropy() {
    let u = Uncertainty { avg_entropy: 3.0, cosine_sim: 0.9, rule_hits: 0, gate_shift: 0.0 };
    let mut state = ArbiterState::default();
    let cfg = ArbiterCfg::default();
    assert_eq!(decide_escalation_cfg(u, &cfg, &mut state), Escalation::CritiquePass);
}

#[test]
fn clean() {
    let u = Uncertainty { avg_entropy: 1.1, cosine_sim: 0.9, rule_hits: 0, gate_shift: 0.0 };
    let mut state = ArbiterState::default();
    let cfg = ArbiterCfg::default();
    assert_eq!(decide_escalation_cfg(u, &cfg, &mut state), Escalation::None);
}

#[test]
fn arbiter_view_weighted_aggregation() {
    let mut view = ArbiterEvidenceView::new("intent-1");

    view.push(Evidence {
        source_id: "llm".to_string(),
        intent_id: "intent-1".to_string(),
        origin: "decoder".to_string(),
        gate_shift: 1.0,
        avg_entropy: 1.0,
        cosine_sim: 1.0,
        rule_hits: 0,
        weight: 1.0,
    });

    view.push(Evidence {
        source_id: "stt".to_string(),
        intent_id: "intent-1".to_string(),
        origin: "prosody".to_string(),
        gate_shift: 3.0,
        avg_entropy: 3.0,
        cosine_sim: 0.0,
        rule_hits: 2,
        weight: 1.0,
    });

    let u = view.to_uncertainty();
    assert!((u.avg_entropy - 2.0).abs() < 1e-6);
    assert!((u.cosine_sim - 0.5).abs() < 1e-6);
    assert!((u.gate_shift - 2.0).abs() < 1e-6);
    assert_eq!(u.rule_hits, 1);
}