//! Opt-in simulator boundary, not yet the C2 weights-backed Session.
//! Never treats a refused prefix as a trainable completed episode.
use sts2_vecenv::{DefaultSim, LaneSim};
use sts2_wire::WireBatch;

/// Original game identity every AI-play policy and trajectory is bound to.
pub const GAME_DLL_SHA256: &str = "0861bfa1df347538d932f22d580e75420f08082792eb914e53b4882764acdbe9";
pub const VANILLA_FINGERPRINT: &str = "be4f87d1704bd6880d1b9d5f8f2989f6ed6b024a0e6a22f168b1e50f4e0820b8";

/// The current native main policy (NativeObsV1 + NativePolicy): what the browser plays.
#[cfg(feature = "native")]
pub mod native;
#[cfg(feature = "native")]
pub mod native_verify;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Canonical CPU observation bytes: field order is WireBatch declaration order,
/// excluding its host-sized lane count. Integers/f32 bits are little endian;
/// masks and booleans occupy exactly one byte. No padding or native pointers.
pub fn observation_bytes(w: &WireBatch) -> Vec<u8> {
    let mut out = Vec::new();
    macro_rules! words {
        ($($field:ident),*) => { $(for v in &w.$field {
            out.extend_from_slice(&v.to_le_bytes());
        })* };
    }
    macro_rules! bytes {
        ($($field:ident),*) => { $(out.extend(w.$field.iter().map(|v| *v as u8));)* };
    }
    words!(state_kind, state_tokens, state_values, state_scales, state_tail);
    bytes!(state_mask);
    words!(boss, cand_kind, cand_tokens, cand_values, cand_scales, cand_tail, choice);
    bytes!(model_mask, source_applicable, target_applicable, source_known, target_known,
        source_bits, target_bits);
    out.extend(w.legal.iter().map(|v| u8::from(*v)));
    out
}

/// Low-level worker-owned simulator. Policy statistics must come from the
/// sampler, not from the simulator; the C2 facade is intentionally not claimed.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct ReplaySession {
    engine: DefaultSim,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl ReplaySession {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(seed: u64) -> Self {
        let mut engine = DefaultSim::new();
        engine.reset_device(seed, 10);
        Self { engine }
    }

    pub fn observation(&self) -> Vec<u8> {
        if self.engine.is_done() { return Vec::new(); }
        observation_bytes(self.engine.observation())
    }

    pub fn legal_actions(&self) -> Vec<u32> {
        (0..sts2_wire::CANDIDATES)
            .filter(|&slot| self.engine.wire_candidate_model(slot as i32) == 1)
            .map(|slot| slot as u32).collect()
    }

    /// Returns false without advancing on invalid caller input.
    pub fn advance(&mut self, action: u32, behavior_logp: f32, value: f32) -> bool {
        if self.engine.is_done() || !behavior_logp.is_finite() || behavior_logp > 0.0
            || !value.is_finite() || action >= sts2_wire::CANDIDATES as u32
            || self.engine.wire_candidate_model(action as i32) != 1 {
            return false;
        }
        self.engine.step_device(action as i32);
        true
    }

    pub fn done(&self) -> bool { self.engine.is_done() }
    pub fn fault(&self) -> String { self.engine.reason_text().unwrap_or("").to_owned() }
    pub fn reward(&self) -> f32 { self.engine.step_reward() }
}

/// Diagnostic first-legal policy (logp=0, V=0), NOT checkpoint self-play.
/// Includes every observation byte, selected action, reward bits and refusal.
/// Bounded independently of episode length; cannot enter a training queue.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn parity_probe(seed: u64, steps: u32) -> Vec<u8> {
    assert!(steps <= 256, "parity probe exceeds decision cap");
    let mut session = ReplaySession::new(seed);
    let mut out = Vec::new();
    for _ in 0..steps {
        let obs = session.observation();
        out.extend_from_slice(&(obs.len() as u32).to_le_bytes());
        out.extend(obs);
        if session.done() { break; }
        let Some(action) = session.legal_actions().first().copied() else { break; };
        out.extend_from_slice(&action.to_le_bytes());
        assert!(session.advance(action, 0.0, 0.0));
        out.extend_from_slice(&session.reward().to_le_bytes());
        out.push(u8::from(session.done()));
    }
    let fault = session.fault();
    out.extend_from_slice(&(fault.len() as u32).to_le_bytes());
    out.extend_from_slice(fault.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_errors_do_not_advance_or_poison_session() {
        let mut s = ReplaySession::new(40);
        assert!(!s.done(), "{}", s.fault());
        let before = s.observation();
        let action = s.legal_actions()[0];
        for (a, logp, value) in [(256, 0.0, 0.0), (action, f32::NAN, 0.0),
            (action, 0.1, 0.0), (action, 0.0, f32::INFINITY)] {
            assert!(!s.advance(a, logp, value));
            assert_eq!(before, s.observation());
            assert!(!s.done());
        }
        assert!(s.advance(action, 0.0, 0.0));
        assert_ne!(before, s.observation());
    }

    #[test]
    fn worker_observations_and_transitions_match_direct_cpu_backend() {
        for seed in [0, 1, 40, 3000040] {
            let mut direct = DefaultSim::new();
            direct.reset_device(seed, 10);
            let mut session = ReplaySession::new(seed);
            for _ in 0..64 {
                assert_eq!(session.done(), direct.is_done());
                assert_eq!(session.fault(), direct.reason_text().unwrap_or(""));
                if direct.is_done() { break; }
                assert_eq!(session.observation(), observation_bytes(direct.observation()));
                let Some(action) = session.legal_actions().first().copied() else { break; };
                direct.step_device(action as i32);
                assert!(session.advance(action, 0.0, 0.0));
                assert_eq!(session.reward().to_bits(), direct.step_reward().to_bits());
            }
        }
    }
}
