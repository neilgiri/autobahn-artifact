use crate::core::RoundNumber;
use config::config::Committee;
use crypto::crypto::PublicKey;

pub type LeaderElector = RRLeaderElector;

pub struct RRLeaderElector {
    committee: Committee,
}

impl RRLeaderElector {
    pub fn new(committee: Committee) -> Self {
        Self { committee }
    }

    pub fn get_leader(&self, round: RoundNumber) -> PublicKey {
        let mut keys: Vec<_> = self.committee.authorities.keys().cloned().collect();
        keys.sort();
        keys[round as usize % self.committee.size()]
    }
}
