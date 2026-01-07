pub mod oddity;
pub mod sources;

pub mod evidence;
pub mod freeze;
pub mod cfg;
pub mod state;
pub mod decide;

pub use oddity::{PersonaBaselines, OddityParams, compute_oddity};
pub use sources::{SourceProfile, SourceProfiles, apply_source_profiles, default_source_profiles};

pub use evidence::{Uncertainty, Evidence, ArbiterEvidenceView};
pub use freeze::{FreezeFlags, freeze_flags};
pub use cfg::ArbiterCfg;
pub use state::ArbiterState;
pub use decide::{Escalation, decide_escalation_cfg, arbiter_idle_tick, decide_escalation_from_view};

// Optional: if you keep hyst_* compatibility shims
pub use state::{hyst_reset, hyst_bump};