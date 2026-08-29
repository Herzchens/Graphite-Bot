BEGIN;

INSERT INTO content_registry_versions (version, label, source_reference)
VALUES (
    2,
    'Graphite frozen content lattice v2 with ordinary smelting recipes',
    'Graphite Master Specification §40 Smelting and Appendix A processed-resource lattice'
);

INSERT INTO content_catalog_entries (
    policy_version, content_key, display_name, content_kind, source_class, metadata
)
SELECT 2, content_key, display_name, content_kind, source_class, metadata
  FROM content_catalog_entries
 WHERE policy_version = 1;

INSERT INTO npc_price_entries (
    policy_version, content_key, appraisal_mode, canonical_appraisal,
    npc_buy_price, npc_liquidation_allowed, shop_sell_price,
    normal_shop_allowed, shop_stock_policy, shop_class
)
SELECT 2, content_key, appraisal_mode, canonical_appraisal,
       npc_buy_price, npc_liquidation_allowed, shop_sell_price,
       normal_shop_allowed, shop_stock_policy, shop_class
  FROM npc_price_entries
 WHERE policy_version = 1;

INSERT INTO content_recipes (
    policy_version, recipe_key, recipe_kind, output_content_key, output_quantity, metadata
)
SELECT 2, recipe_key, recipe_kind, output_content_key, output_quantity, metadata
  FROM content_recipes
 WHERE policy_version = 1;

INSERT INTO content_recipe_inputs (
    policy_version, recipe_key, sequence, content_key, quantity
)
SELECT 2, recipe_key, sequence, content_key, quantity
  FROM content_recipe_inputs
 WHERE policy_version = 1;

-- Recipe rows own only versioned input/output mapping. Global ordinary-Smelting
-- timing, heat, stop/cancel, and AEXP policy has one source of truth in
-- graphite-services instead of being duplicated into each recipe's JSON metadata.
INSERT INTO content_recipes (
    policy_version, recipe_key, recipe_kind, output_content_key, output_quantity, metadata
)
VALUES
    (2, 'smelt.tin', 'SMELT', 'resource.ingot.tin', 1, '{}'::jsonb),
    (2, 'smelt.copper', 'SMELT', 'resource.ingot.copper', 1, '{}'::jsonb),
    (2, 'smelt.zinc', 'SMELT', 'resource.ingot.zinc', 1, '{}'::jsonb),
    (2, 'smelt.aluminum', 'SMELT', 'resource.ingot.aluminum', 1, '{}'::jsonb),
    (2, 'smelt.iron', 'SMELT', 'resource.ingot.iron', 1, '{}'::jsonb),
    (2, 'smelt.lead', 'SMELT', 'resource.ingot.lead', 1, '{}'::jsonb),
    (2, 'smelt.silver', 'SMELT', 'resource.ingot.silver', 1, '{}'::jsonb),
    (2, 'smelt.nickel', 'SMELT', 'resource.ingot.nickel', 1, '{}'::jsonb),
    (2, 'smelt.gold', 'SMELT', 'resource.ingot.gold', 1, '{}'::jsonb),
    (2, 'smelt.cobalt', 'SMELT', 'resource.ingot.cobalt', 1, '{}'::jsonb),
    (2, 'smelt.titanium', 'SMELT', 'resource.ingot.titanium', 1, '{}'::jsonb),
    (2, 'smelt.tungsten', 'SMELT', 'resource.ingot.tungsten', 1, '{}'::jsonb),
    (2, 'smelt.netherite-scrap', 'SMELT', 'resource.netherite_scrap', 1, '{}'::jsonb),
    (2, 'smelt.platinum', 'SMELT', 'resource.ingot.platinum', 1, '{}'::jsonb);

INSERT INTO content_recipe_inputs (
    policy_version, recipe_key, sequence, content_key, quantity
)
VALUES
    (2, 'smelt.tin', 1, 'resource.ore.tin', 1),
    (2, 'smelt.copper', 1, 'resource.ore.copper', 1),
    (2, 'smelt.zinc', 1, 'resource.ore.zinc', 1),
    (2, 'smelt.aluminum', 1, 'resource.bauxite', 1),
    (2, 'smelt.iron', 1, 'resource.ore.iron', 1),
    (2, 'smelt.lead', 1, 'resource.ore.lead', 1),
    (2, 'smelt.silver', 1, 'resource.ore.silver', 1),
    (2, 'smelt.nickel', 1, 'resource.ore.nickel', 1),
    (2, 'smelt.gold', 1, 'resource.ore.gold', 1),
    (2, 'smelt.cobalt', 1, 'resource.ore.cobalt', 1),
    (2, 'smelt.titanium', 1, 'resource.ore.titanium', 1),
    (2, 'smelt.tungsten', 1, 'resource.ore.tungsten', 1),
    (2, 'smelt.netherite-scrap', 1, 'resource.ancient_debris', 1),
    (2, 'smelt.platinum', 1, 'resource.ore.platinum', 1);

UPDATE active_content_registry
   SET version = 2,
       activated_at = now()
 WHERE singleton = TRUE;

COMMIT;
