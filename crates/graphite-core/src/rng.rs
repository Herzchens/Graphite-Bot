use blake3::Hasher;
use rand_chacha::{
    ChaCha12Rng,
    rand_core::{Rng, SeedableRng},
};
use thiserror::Error;

use crate::OperationId;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct RootSeed([u8; 32]);

impl RootSeed {
    #[must_use]
    pub fn generate() -> Self {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).expect("OS CSPRNG must be available");
        Self(bytes)
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum RngError {
    #[error("weighted sample requires at least one positive weight")]
    EmptyDistribution,
    #[error("weight total overflow")]
    WeightOverflow,
}

pub struct DomainRng(ChaCha12Rng);

impl DomainRng {
    #[must_use]
    pub fn derive(root: RootSeed, operation_id: OperationId, domain: &str) -> Self {
        let mut hasher = Hasher::new_keyed(root.as_bytes());
        hasher.update(b"graphite.rng.v1\0");
        hasher.update(operation_id.as_uuid().as_bytes());
        hasher.update(b"\0");
        hasher.update(domain.as_bytes());
        let seed = *hasher.finalize().as_bytes();
        Self(ChaCha12Rng::from_seed(seed))
    }

    pub fn sample_weighted(&mut self, weights: &[u64]) -> Result<usize, RngError> {
        let total = weights.iter().try_fold(0_u64, |acc, &weight| {
            acc.checked_add(weight).ok_or(RngError::WeightOverflow)
        })?;
        if total == 0 {
            return Err(RngError::EmptyDistribution);
        }

        let rejection_threshold = u64::MAX - (u64::MAX % total);
        let draw = loop {
            let candidate = self.0.next_u64();
            if candidate < rejection_threshold {
                break candidate % total;
            }
        };

        let mut remaining = draw;
        for (index, &weight) in weights.iter().enumerate() {
            if remaining < weight {
                return Ok(index);
            }
            remaining -= weight;
        }

        unreachable!("draw is strictly below the checked weight total");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn fixed_operation() -> OperationId {
        OperationId::from_uuid(Uuid::parse_str("018f3a2f-21a0-7b4b-8a44-1a41d87e3cf5").unwrap())
    }

    #[test]
    fn replay_is_deterministic_for_same_operation_and_domain() {
        let root = RootSeed::from_bytes([7; 32]);
        let mut a = DomainRng::derive(root, fixed_operation(), "mining.ore");
        let mut b = DomainRng::derive(root, fixed_operation(), "mining.ore");
        let weights = [1, 4, 9, 2];

        let left: Vec<_> = (0..64)
            .map(|_| a.sample_weighted(&weights).unwrap())
            .collect();
        let right: Vec<_> = (0..64)
            .map(|_| b.sample_weighted(&weights).unwrap())
            .collect();
        assert_eq!(left, right);
    }

    #[test]
    fn cosmetic_domain_does_not_perturb_loot_domain() {
        let root = RootSeed::from_bytes([3; 32]);
        let mut ore_a = DomainRng::derive(root, fixed_operation(), "mining.ore");
        let mut cosmetic = DomainRng::derive(root, fixed_operation(), "cosmetic");
        let _ = (0..100)
            .map(|_| cosmetic.sample_weighted(&[1, 1, 1]).unwrap())
            .collect::<Vec<_>>();
        let mut ore_b = DomainRng::derive(root, fixed_operation(), "mining.ore");

        for _ in 0..100 {
            assert_eq!(
                ore_a.sample_weighted(&[2, 7, 5]),
                ore_b.sample_weighted(&[2, 7, 5])
            );
        }
    }
}
