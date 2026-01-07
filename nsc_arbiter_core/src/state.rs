use crate::freeze::FreezeFlags;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ArbiterState {
    pub hyst_rep: u32,
    pub hyst_stall: u32,
}

impl ArbiterState {
    #[inline]
    pub fn bump(&mut self, ff: FreezeFlags, disable: bool) {
        if disable { return; }
        if ff.rep_3p { self.hyst_rep += 1; }
        if ff.stall  { self.hyst_stall += 1; }
    }

    #[inline]
    pub fn reset(&mut self) {
        self.hyst_rep = 0;
        self.hyst_stall = 0;
    }
}

// compat shims (optional)
pub fn hyst_reset() { }
pub fn hyst_bump(_ff: FreezeFlags) { }