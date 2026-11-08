use crate::{rng::Rng, run::Coord};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapVote { pub generation: u32, pub coord: Coord }

/// MapSelectionSynchronizer: every roster slot votes, then sample a player's
/// vote (not a majority and not a deduplicated list of destinations).
#[derive(Clone, Debug)]
pub struct MapVoting {
    pub rng: Rng,
    pub generation: u32,
    location: (usize, Coord),
    votes: Vec<Option<MapVote>>,
}

impl MapVoting {
    pub fn new(seed: &str, seats: usize, act: usize, coord: Coord) -> Result<Self, &'static str> {
        Self::from_numeric_seed(super::rng::decoded_run_seed(seed), seats, act, coord)
    }

    pub fn from_numeric_seed(seed: u64, seats: usize, act: usize, coord: Coord) -> Result<Self, &'static str> {
        if !(2..=4).contains(&seats) { return Err("map voting requires 2-4 seats"); }
        Ok(Self { rng: Rng::named(seed, "map_point_selection"),
            generation: 0, location: (act, coord), votes: vec![None; seats] })
    }

    pub fn before_map_generated(&mut self) {
        self.generation += 1;
        for vote in &mut self.votes {
            if vote.is_some_and(|v| v.generation < self.generation) { *vote = None; }
        }
    }

    pub fn on_location_changed(&mut self, act: usize, coord: Coord) {
        self.location = (act, coord);
        self.votes.fill(None);
    }

    pub fn votes(&self) -> &[Option<MapVote>] { &self.votes }

    /// Caller must validate the actor and legal map edges before submitting.
    /// Original stale-source/stale-generation messages are ignored.
    pub fn vote(&mut self, seat: usize, source: (usize, Coord), destination: Option<MapVote>)
        -> Result<Option<Coord>, &'static str> {
        if seat >= self.votes.len() { return Err("invalid map voter"); }
        if source != self.location || destination.is_some_and(|v| v.generation < self.generation) {
            return Ok(None);
        }
        self.votes[seat] = destination;
        if !self.votes.iter().all(|v| v.is_some_and(|v| v.generation == self.generation)) {
            return Ok(None);
        }
        let coord = self.votes[self.rng.choice_index(self.votes.len())].unwrap().coord;
        // Original updates accepting source before its queued move executes.
        self.location.1 = coord;
        Ok(Some(coord))
    }
}
