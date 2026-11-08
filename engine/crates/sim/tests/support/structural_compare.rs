//! Boundary comparator: adapters must supply canonical bytes, not Debug output,
//! hashes, or summaries. RNG bytes include every stream's words and counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Match,
    Fault { decision: usize, reason: String },
    Mismatch { decision: usize, reason: String },
    CSharp { decision: usize, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub actions: Vec<u8>,
    pub obs: Vec<u8>,
    pub state: Vec<u8>,
    pub rng: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub frames: Vec<Frame>,
    pub outcome: Outcome,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Difference {
    Bytes { decision: usize, field: &'static str, offset: usize },
    FrameCount { baseline: usize, candidate: usize },
    Outcome { baseline: Outcome, candidate: Outcome },
}

pub fn compare(baseline: &Run, candidate: &Run) -> Result<(), Vec<Difference>> {
    let mut differences = vec![];
    for (decision, (a, b)) in baseline.frames.iter().zip(&candidate.frames).enumerate() {
        for (field, a, b) in [
            ("actions", &a.actions, &b.actions), ("obs", &a.obs, &b.obs),
            ("state", &a.state, &b.state), ("rng", &a.rng, &b.rng),
        ] {
            if a != b {
                let offset = a.iter().zip(b).position(|(a, b)| a != b).unwrap_or(a.len().min(b.len()));
                differences.push(Difference::Bytes { decision, field, offset });
            }
        }
    }
    if baseline.frames.len() != candidate.frames.len() {
        differences.push(Difference::FrameCount { baseline: baseline.frames.len(), candidate: candidate.frames.len() });
    }
    // Never collapse mismatches, unsupported faults or accepted C# divergences.
    if baseline.outcome != candidate.outcome {
        differences.push(Difference::Outcome { baseline: baseline.outcome.clone(), candidate: candidate.outcome.clone() });
    }
    if differences.is_empty() { Ok(()) } else { Err(differences) }
}
