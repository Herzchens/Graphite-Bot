use crate::equipment_policy::EquipmentTier;
use serde::Serialize;
use thiserror::Error;

const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_HOUR: i64 = 60 * SECONDS_PER_MINUTE;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdvancedForgeStackRecipe {
    NetheriteBillet,
    GraphitePrecursor,
    GraphiteLayer,
    GraphiteBillet,
}

impl AdvancedForgeStackRecipe {
    pub const fn content_recipe_key(self) -> &'static str {
        match self {
            Self::NetheriteBillet => "forge.netherite-billet",
            Self::GraphitePrecursor => "forge.graphite-precursor",
            Self::GraphiteLayer => "forge.graphite-layer",
            Self::GraphiteBillet => "forge.graphite-billet",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ForgeSuccessChance {
    numerator: u32,
    denominator: u32,
}

impl ForgeSuccessChance {
    const GUARANTEED: Self = Self {
        numerator: 1,
        denominator: 1,
    };
    const GRAPHITE_LAYER: Self = Self {
        numerator: 2,
        denominator: 5,
    };

    pub const fn numerator(self) -> u32 {
        self.numerator
    }

    pub const fn denominator(self) -> u32 {
        self.denominator
    }

    pub const fn is_guaranteed(self) -> bool {
        self.numerator == self.denominator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ForgePostConfirmCancellation {
    /// The active specification leaves this ordinary Forge recipe's cancellation policy to its
    /// owning recipe/service slice. Callers must not interpret this as cancellable.
    Unspecified,
    Forbidden,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ForgeFailurePolicy {
    Impossible,
    /// Failure consumes the committed precursor/cost, has no pity, and is not modified/refunded by
    /// Sparkling, Stabilize, Protection Orb, Enchant Catalyst, or Mosaic.
    ConsumeCommittedInputsNoPityNoUpgradeModifiers,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AdvancedForgeStackPolicy {
    pub recipe: AdvancedForgeStackRecipe,
    pub content_recipe_key: &'static str,
    pub money_cost: i64,
    pub activity_xp_cost: i64,
    pub duration_seconds: i64,
    pub success: ForgeSuccessChance,
    pub cancellation_after_confirm: ForgePostConfirmCancellation,
    pub failure_policy: ForgeFailurePolicy,
}

/// Returns the frozen economic/runtime policy for an advanced stack-output Forge recipe.
///
/// Versioned stack input/output mapping remains authoritative in the content registry under
/// `content_recipe_key`; this function intentionally does not duplicate those quantities or choose
/// ItemDefinition identities. It also does not reserve assets, draw RNG, create a job, or commit an
/// operation.
pub const fn advanced_forge_stack_policy(
    recipe: AdvancedForgeStackRecipe,
) -> AdvancedForgeStackPolicy {
    match recipe {
        AdvancedForgeStackRecipe::NetheriteBillet => AdvancedForgeStackPolicy {
            recipe,
            content_recipe_key: recipe.content_recipe_key(),
            money_cost: 5_000,
            activity_xp_cost: 1_000,
            duration_seconds: 15 * SECONDS_PER_MINUTE,
            success: ForgeSuccessChance::GUARANTEED,
            cancellation_after_confirm: ForgePostConfirmCancellation::Unspecified,
            failure_policy: ForgeFailurePolicy::Impossible,
        },
        AdvancedForgeStackRecipe::GraphitePrecursor => AdvancedForgeStackPolicy {
            recipe,
            content_recipe_key: recipe.content_recipe_key(),
            money_cost: 15_000,
            activity_xp_cost: 1_000,
            duration_seconds: 10 * SECONDS_PER_MINUTE,
            success: ForgeSuccessChance::GUARANTEED,
            cancellation_after_confirm: ForgePostConfirmCancellation::Forbidden,
            failure_policy: ForgeFailurePolicy::Impossible,
        },
        AdvancedForgeStackRecipe::GraphiteLayer => AdvancedForgeStackPolicy {
            recipe,
            content_recipe_key: recipe.content_recipe_key(),
            money_cost: 5_000,
            activity_xp_cost: 500,
            duration_seconds: 30 * SECONDS_PER_MINUTE,
            success: ForgeSuccessChance::GRAPHITE_LAYER,
            cancellation_after_confirm: ForgePostConfirmCancellation::Forbidden,
            failure_policy: ForgeFailurePolicy::ConsumeCommittedInputsNoPityNoUpgradeModifiers,
        },
        AdvancedForgeStackRecipe::GraphiteBillet => AdvancedForgeStackPolicy {
            recipe,
            content_recipe_key: recipe.content_recipe_key(),
            money_cost: 500_000,
            activity_xp_cost: 25_000,
            duration_seconds: 2 * SECONDS_PER_HOUR,
            success: ForgeSuccessChance::GUARANTEED,
            cancellation_after_confirm: ForgePostConfirmCancellation::Forbidden,
            failure_policy: ForgeFailurePolicy::Impossible,
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdvancedForgePromotion {
    ObsidianToNetherite,
    NetheriteToGraphite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AdvancedForgePromotionPolicy {
    pub promotion: AdvancedForgePromotion,
    pub from_tier: EquipmentTier,
    pub to_tier: EquipmentTier,
    pub required_component_content_key: &'static str,
    pub required_component_quantity: i64,
    pub money_cost: i64,
    pub activity_xp_cost: i64,
    pub duration_seconds: i64,
    pub success: ForgeSuccessChance,
    pub cancellation_after_confirm: ForgePostConfirmCancellation,
    pub bound_item_requires_soulbind_top_up: bool,
}

/// Returns frozen policy for a same-ItemInstance advanced tier promotion.
///
/// Both promotions preserve the existing ItemInstance UUID, +N, creation roll, valid enchants, and
/// unlocked slots. The caller must separately enforce eligibility and compatibility and must apply
/// [`project_promoted_durability`] using the authoritative old/new maximum durability. A SoulBound
/// Netherite→Graphite target additionally requires the ordinary appraisal-delta SoulBind top-up in
/// the owning atomic transaction; this module deliberately does not compute that still-stateful
/// charge.
pub const fn advanced_forge_promotion_policy(
    promotion: AdvancedForgePromotion,
) -> AdvancedForgePromotionPolicy {
    match promotion {
        AdvancedForgePromotion::ObsidianToNetherite => AdvancedForgePromotionPolicy {
            promotion,
            from_tier: EquipmentTier::Obsidian,
            to_tier: EquipmentTier::Netherite,
            required_component_content_key: "material.netherite_billet",
            required_component_quantity: 1,
            money_cost: 150_000,
            activity_xp_cost: 5_000,
            duration_seconds: SECONDS_PER_HOUR,
            success: ForgeSuccessChance::GUARANTEED,
            cancellation_after_confirm: ForgePostConfirmCancellation::Unspecified,
            bound_item_requires_soulbind_top_up: false,
        },
        AdvancedForgePromotion::NetheriteToGraphite => AdvancedForgePromotionPolicy {
            promotion,
            from_tier: EquipmentTier::Netherite,
            to_tier: EquipmentTier::Graphite,
            required_component_content_key: "material.graphite_billet",
            required_component_quantity: 1,
            money_cost: 1_800_000,
            activity_xp_cost: 50_000,
            duration_seconds: 4 * SECONDS_PER_HOUR,
            success: ForgeSuccessChance::GUARANTEED,
            cancellation_after_confirm: ForgePostConfirmCancellation::Forbidden,
            bound_item_requires_soulbind_top_up: true,
        },
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ForgePolicyError {
    #[error("current durability must be between zero and the old maximum durability")]
    InvalidCurrentDurability,
    #[error("old maximum durability must be positive")]
    InvalidOldMaxDurability,
    #[error("promoted maximum durability must be positive")]
    InvalidPromotedMaxDurability,
    #[error("Forge durability arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Projects current durability across a same-ItemInstance tier promotion.
///
/// The frozen rule is `floor(old_current / old_max × promoted_max)`. A Broken/zero-durability item
/// therefore remains at zero. This function assumes any separate Broken status is preserved by the
/// owning ItemInstance mutation and intentionally does not mutate persistent state.
pub fn project_promoted_durability(
    old_current: i64,
    old_max: i64,
    promoted_max: i64,
) -> Result<i64, ForgePolicyError> {
    if old_max <= 0 {
        return Err(ForgePolicyError::InvalidOldMaxDurability);
    }
    if promoted_max <= 0 {
        return Err(ForgePolicyError::InvalidPromotedMaxDurability);
    }
    if old_current < 0 || old_current > old_max {
        return Err(ForgePolicyError::InvalidCurrentDurability);
    }

    let projected = i128::from(old_current)
        .checked_mul(i128::from(promoted_max))
        .ok_or(ForgePolicyError::ArithmeticOverflow)?
        / i128::from(old_max);
    i64::try_from(projected).map_err(|_| ForgePolicyError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_recipe_policy_matches_frozen_advanced_forge_values() {
        let cases = [
            (
                AdvancedForgeStackRecipe::NetheriteBillet,
                "forge.netherite-billet",
                5_000,
                1_000,
                15 * SECONDS_PER_MINUTE,
                ForgeSuccessChance::GUARANTEED,
                ForgePostConfirmCancellation::Unspecified,
                ForgeFailurePolicy::Impossible,
            ),
            (
                AdvancedForgeStackRecipe::GraphitePrecursor,
                "forge.graphite-precursor",
                15_000,
                1_000,
                10 * SECONDS_PER_MINUTE,
                ForgeSuccessChance::GUARANTEED,
                ForgePostConfirmCancellation::Forbidden,
                ForgeFailurePolicy::Impossible,
            ),
            (
                AdvancedForgeStackRecipe::GraphiteLayer,
                "forge.graphite-layer",
                5_000,
                500,
                30 * SECONDS_PER_MINUTE,
                ForgeSuccessChance::GRAPHITE_LAYER,
                ForgePostConfirmCancellation::Forbidden,
                ForgeFailurePolicy::ConsumeCommittedInputsNoPityNoUpgradeModifiers,
            ),
            (
                AdvancedForgeStackRecipe::GraphiteBillet,
                "forge.graphite-billet",
                500_000,
                25_000,
                2 * SECONDS_PER_HOUR,
                ForgeSuccessChance::GUARANTEED,
                ForgePostConfirmCancellation::Forbidden,
                ForgeFailurePolicy::Impossible,
            ),
        ];

        for (recipe, key, money, aexp, duration, success, cancellation, failure) in cases {
            let policy = advanced_forge_stack_policy(recipe);
            assert_eq!(policy.content_recipe_key, key);
            assert_eq!(policy.money_cost, money);
            assert_eq!(policy.activity_xp_cost, aexp);
            assert_eq!(policy.duration_seconds, duration);
            assert_eq!(policy.success, success);
            assert_eq!(policy.cancellation_after_confirm, cancellation);
            assert_eq!(policy.failure_policy, failure);
        }
    }

    #[test]
    fn graphite_layer_chance_is_exact_two_fifths_without_rng_mapping() {
        let policy = advanced_forge_stack_policy(AdvancedForgeStackRecipe::GraphiteLayer);
        assert_eq!(policy.success.numerator(), 2);
        assert_eq!(policy.success.denominator(), 5);
        assert!(!policy.success.is_guaranteed());
        assert_eq!(
            policy.failure_policy,
            ForgeFailurePolicy::ConsumeCommittedInputsNoPityNoUpgradeModifiers
        );
    }

    #[test]
    fn guaranteed_policies_expose_canonical_unit_fraction() {
        for success in [
            advanced_forge_stack_policy(AdvancedForgeStackRecipe::NetheriteBillet).success,
            advanced_forge_stack_policy(AdvancedForgeStackRecipe::GraphitePrecursor).success,
            advanced_forge_stack_policy(AdvancedForgeStackRecipe::GraphiteBillet).success,
            advanced_forge_promotion_policy(AdvancedForgePromotion::ObsidianToNetherite).success,
            advanced_forge_promotion_policy(AdvancedForgePromotion::NetheriteToGraphite).success,
        ] {
            assert_eq!(success.numerator(), 1);
            assert_eq!(success.denominator(), 1);
            assert!(success.is_guaranteed());
        }
    }

    #[test]
    fn promotion_policy_matches_frozen_chain() {
        let netherite =
            advanced_forge_promotion_policy(AdvancedForgePromotion::ObsidianToNetherite);
        assert_eq!(netherite.from_tier, EquipmentTier::Obsidian);
        assert_eq!(netherite.to_tier, EquipmentTier::Netherite);
        assert_eq!(
            netherite.required_component_content_key,
            "material.netherite_billet"
        );
        assert_eq!(netherite.required_component_quantity, 1);
        assert_eq!(netherite.money_cost, 150_000);
        assert_eq!(netherite.activity_xp_cost, 5_000);
        assert_eq!(netherite.duration_seconds, SECONDS_PER_HOUR);
        assert_eq!(netherite.success, ForgeSuccessChance::GUARANTEED);
        assert_eq!(
            netherite.cancellation_after_confirm,
            ForgePostConfirmCancellation::Unspecified
        );
        assert!(!netherite.bound_item_requires_soulbind_top_up);

        let graphite = advanced_forge_promotion_policy(AdvancedForgePromotion::NetheriteToGraphite);
        assert_eq!(graphite.from_tier, EquipmentTier::Netherite);
        assert_eq!(graphite.to_tier, EquipmentTier::Graphite);
        assert_eq!(
            graphite.required_component_content_key,
            "material.graphite_billet"
        );
        assert_eq!(graphite.required_component_quantity, 1);
        assert_eq!(graphite.money_cost, 1_800_000);
        assert_eq!(graphite.activity_xp_cost, 50_000);
        assert_eq!(graphite.duration_seconds, 4 * SECONDS_PER_HOUR);
        assert_eq!(graphite.success, ForgeSuccessChance::GUARANTEED);
        assert_eq!(
            graphite.cancellation_after_confirm,
            ForgePostConfirmCancellation::Forbidden
        );
        assert!(graphite.bound_item_requires_soulbind_top_up);
    }

    #[test]
    fn durability_projection_preserves_floor_ratio_and_broken_zero() {
        assert_eq!(project_promoted_durability(50, 100, 250).unwrap(), 125);
        assert_eq!(project_promoted_durability(1, 3, 100).unwrap(), 33);
        assert_eq!(project_promoted_durability(2, 3, 100).unwrap(), 66);
        assert_eq!(project_promoted_durability(0, 100, 999).unwrap(), 0);
        assert_eq!(project_promoted_durability(100, 100, 999).unwrap(), 999);
    }

    #[test]
    fn durability_projection_rejects_invalid_boundaries() {
        assert_eq!(
            project_promoted_durability(0, 0, 100),
            Err(ForgePolicyError::InvalidOldMaxDurability)
        );
        assert_eq!(
            project_promoted_durability(0, 100, 0),
            Err(ForgePolicyError::InvalidPromotedMaxDurability)
        );
        assert_eq!(
            project_promoted_durability(-1, 100, 100),
            Err(ForgePolicyError::InvalidCurrentDurability)
        );
        assert_eq!(
            project_promoted_durability(101, 100, 100),
            Err(ForgePolicyError::InvalidCurrentDurability)
        );
    }
}
