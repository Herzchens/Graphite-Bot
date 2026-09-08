use serde::Serialize;
use thiserror::Error;

pub const NORMAL_PICKAXE_DURABILITY_PER_ORDINARY_EVENT: u8 = 1;
pub const NUKE_MAX_BLAST_PHYSICAL_BLOCKS: u32 = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MiningPickaxeOrdinaryDurabilityConsequence {
    WearApplied,
    WearPreventedByUnbreaking,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MiningPickaxeOrdinaryDurabilityPreview {
    pub current_durability: u32,
    pub max_durability: u32,
    pub resulting_durability: u32,
    pub consequence: MiningPickaxeOrdinaryDurabilityConsequence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MiningPickaxeNukeDestructionPreview {
    pub remaining_durability: u32,
    pub max_durability: u32,
    pub blast_physical_blocks: u32,
    pub resulting_durability: u32,
    pub nuke_burnout_required: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum MiningPickaxeDurabilityPolicyError {
    #[error("Pickaxe definition is not an ordinary Pickaxe")]
    NotOrdinaryPickaxe,
    #[error("maximum Pickaxe durability must be positive")]
    InvalidMaxDurability,
    #[error(
        "current Pickaxe durability for an ordinary event must be between 1 and maximum durability"
    )]
    InvalidCurrentDurability,
    #[error("remaining Pickaxe durability for Nuke cannot exceed maximum durability")]
    InvalidNukeRemainingDurability,
}

/// Previews one already-resolved ordinary Pickaxe durability event.
///
/// The frozen Mining contract gives one normal Pickaxe durability event to one ordinary manual
/// mining action; Auto Miner likewise pays one such event per ordinary work unit. Fortune-created
/// quantity and Trench-generated blocks do not create additional Pickaxe wear. This function owns
/// only the consequence after the future Mining lifecycle has already resolved whether Unbreaking
/// prevented that event.
///
/// `is_ordinary_pickaxe` must come from authoritative versioned ItemDefinition/ItemInstance state.
/// The system-bound Starter Wood Pickaxe is unbreakable and must not enter this ordinary policy just
/// because it carries Wood-like metadata.
///
/// This function does not infer the Unbreaking level curve or probability, draw RNG, apply Mending,
/// resolve Nuke, mutate an ItemInstance, settle an expedition, or expose `/mine`.
pub const fn preview_mining_pickaxe_ordinary_durability_event(
    current_durability: u32,
    max_durability: u32,
    is_ordinary_pickaxe: bool,
    ordinary_event_prevented_by_unbreaking: bool,
) -> Result<MiningPickaxeOrdinaryDurabilityPreview, MiningPickaxeDurabilityPolicyError> {
    if !is_ordinary_pickaxe {
        return Err(MiningPickaxeDurabilityPolicyError::NotOrdinaryPickaxe);
    }
    if max_durability == 0 {
        return Err(MiningPickaxeDurabilityPolicyError::InvalidMaxDurability);
    }
    if current_durability == 0 || current_durability > max_durability {
        return Err(MiningPickaxeDurabilityPolicyError::InvalidCurrentDurability);
    }

    let (resulting_durability, consequence) = if ordinary_event_prevented_by_unbreaking {
        (
            current_durability,
            MiningPickaxeOrdinaryDurabilityConsequence::WearPreventedByUnbreaking,
        )
    } else {
        (
            current_durability - u32::from(NORMAL_PICKAXE_DURABILITY_PER_ORDINARY_EVENT),
            MiningPickaxeOrdinaryDurabilityConsequence::WearApplied,
        )
    };

    Ok(MiningPickaxeOrdinaryDurabilityPreview {
        current_durability,
        max_durability,
        resulting_durability,
        consequence,
    })
}

/// Previews the frozen destructive consequence of an already-resolved successful Nuke proc.
///
/// Nuke is disabled in Automation, so only the future Manual Mining expedition owner may enter this
/// consequence after it has authoritatively resolved a successful Nuke proc.
///
/// `remaining_durability` is deliberately an input rather than being derived here. The active Master
/// defines Nuke as `blast min(remaining durability, 100) blocks`, then Pickaxe durability becomes
/// zero and `NUKE_BURNOUT` is required, but it does not freeze this primitive's ordering relative to
/// the ordinary durability event of the same `/mine`. The future expedition owner must therefore
/// resolve that ordering and pass the authoritative remaining value at the Nuke step.
///
/// Zero remaining durability is mathematically accepted so this pure formula does not invent a
/// lifecycle ordering rule. It does not authorize a broken Pickaxe to start or continue an
/// expedition; operational-equipment validation remains with the stateful owner.
///
/// Nuke ignores Unbreaking. This function performs no RNG, fall/collapse resolution, geological
/// pressure mutation, Mending/repair, ItemInstance write, operation finalization, or command wiring.
pub const fn preview_mining_pickaxe_nuke_destruction(
    remaining_durability: u32,
    max_durability: u32,
    is_ordinary_pickaxe: bool,
) -> Result<MiningPickaxeNukeDestructionPreview, MiningPickaxeDurabilityPolicyError> {
    if !is_ordinary_pickaxe {
        return Err(MiningPickaxeDurabilityPolicyError::NotOrdinaryPickaxe);
    }
    if max_durability == 0 {
        return Err(MiningPickaxeDurabilityPolicyError::InvalidMaxDurability);
    }
    if remaining_durability > max_durability {
        return Err(MiningPickaxeDurabilityPolicyError::InvalidNukeRemainingDurability);
    }

    Ok(MiningPickaxeNukeDestructionPreview {
        remaining_durability,
        max_durability,
        blast_physical_blocks: remaining_durability.min(NUKE_MAX_BLAST_PHYSICAL_BLOCKS),
        resulting_durability: 0,
        nuke_burnout_required: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_event_consumes_exactly_one_durability() {
        let preview =
            preview_mining_pickaxe_ordinary_durability_event(17, 100, true, false).unwrap();
        assert_eq!(preview.resulting_durability, 16);
        assert_eq!(
            preview.consequence,
            MiningPickaxeOrdinaryDurabilityConsequence::WearApplied
        );
    }

    #[test]
    fn authoritative_unbreaking_prevention_preserves_ordinary_durability() {
        let preview =
            preview_mining_pickaxe_ordinary_durability_event(17, 100, true, true).unwrap();
        assert_eq!(preview.resulting_durability, 17);
        assert_eq!(
            preview.consequence,
            MiningPickaxeOrdinaryDurabilityConsequence::WearPreventedByUnbreaking
        );
    }

    #[test]
    fn one_durability_can_be_consumed_to_zero() {
        assert_eq!(
            preview_mining_pickaxe_ordinary_durability_event(1, 100, true, false)
                .unwrap()
                .resulting_durability,
            0
        );
    }

    #[test]
    fn nuke_caps_blast_at_one_hundred_then_destroys_pickaxe() {
        for (remaining, expected_blocks) in [
            (0, 0),
            (1, 1),
            (99, 99),
            (100, 100),
            (101, 100),
            (11_000, 100),
        ] {
            let preview = preview_mining_pickaxe_nuke_destruction(remaining, 11_000, true).unwrap();
            assert_eq!(preview.blast_physical_blocks, expected_blocks);
            assert_eq!(preview.resulting_durability, 0);
            assert!(preview.nuke_burnout_required);
        }
    }

    #[test]
    fn starter_or_other_nonordinary_pickaxes_fail_closed() {
        assert_eq!(
            preview_mining_pickaxe_ordinary_durability_event(1, 1, false, false),
            Err(MiningPickaxeDurabilityPolicyError::NotOrdinaryPickaxe)
        );
        assert_eq!(
            preview_mining_pickaxe_nuke_destruction(1, 1, false),
            Err(MiningPickaxeDurabilityPolicyError::NotOrdinaryPickaxe)
        );
    }

    #[test]
    fn malformed_durability_fails_closed_without_inventing_nuke_order() {
        assert_eq!(
            preview_mining_pickaxe_ordinary_durability_event(1, 0, true, false),
            Err(MiningPickaxeDurabilityPolicyError::InvalidMaxDurability)
        );
        assert_eq!(
            preview_mining_pickaxe_ordinary_durability_event(0, 100, true, false),
            Err(MiningPickaxeDurabilityPolicyError::InvalidCurrentDurability)
        );
        assert_eq!(
            preview_mining_pickaxe_ordinary_durability_event(101, 100, true, false),
            Err(MiningPickaxeDurabilityPolicyError::InvalidCurrentDurability)
        );
        assert_eq!(
            preview_mining_pickaxe_nuke_destruction(1, 0, true),
            Err(MiningPickaxeDurabilityPolicyError::InvalidMaxDurability)
        );
        assert_eq!(
            preview_mining_pickaxe_nuke_destruction(101, 100, true),
            Err(MiningPickaxeDurabilityPolicyError::InvalidNukeRemainingDurability)
        );
    }
}
