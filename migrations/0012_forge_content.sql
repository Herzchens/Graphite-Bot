BEGIN;

INSERT INTO content_registry_versions (version, label, source_reference)
VALUES (
    3,
    'Graphite frozen content lattice v3 with advanced Forge stack recipes',
    'Graphite Master Specification §41.1 Netherite material path and §41.2 Graphite material path'
);

INSERT INTO content_catalog_entries (
    policy_version, content_key, display_name, content_kind, source_class, metadata
)
SELECT 3, content_key, display_name, content_kind, source_class, metadata
  FROM content_catalog_entries
 WHERE policy_version = 2;

INSERT INTO npc_price_entries (
    policy_version, content_key, appraisal_mode, canonical_appraisal,
    npc_buy_price, npc_liquidation_allowed, shop_sell_price,
    normal_shop_allowed, shop_stock_policy, shop_class
)
SELECT 3, content_key, appraisal_mode, canonical_appraisal,
       npc_buy_price, npc_liquidation_allowed, shop_sell_price,
       normal_shop_allowed, shop_stock_policy, shop_class
  FROM npc_price_entries
 WHERE policy_version = 2;

INSERT INTO content_recipes (
    policy_version, recipe_key, recipe_kind, output_content_key, output_quantity, metadata
)
SELECT 3, recipe_key, recipe_kind, output_content_key, output_quantity, metadata
  FROM content_recipes
 WHERE policy_version = 2;

INSERT INTO content_recipe_inputs (
    policy_version, recipe_key, sequence, content_key, quantity
)
SELECT 3, recipe_key, sequence, content_key, quantity
  FROM content_recipe_inputs
 WHERE policy_version = 2;

-- Recipe rows own only versioned stack input/output mapping. Money, Activity EXP,
-- timing, success/failure, cancellation, policy snapshotting, and same-ItemInstance
-- promotion semantics live in graphite-services rather than duplicated JSON metadata.
INSERT INTO content_recipes (
    policy_version, recipe_key, recipe_kind, output_content_key, output_quantity, metadata
)
VALUES
    (3, 'forge.netherite-billet', 'FORGE', 'material.netherite_billet', 1, '{}'::jsonb),
    (3, 'forge.graphite-precursor', 'FORGE', 'material.graphitic_precursor', 1, '{}'::jsonb),
    (3, 'forge.graphite-layer', 'FORGE', 'material.graphite_layer', 1, '{}'::jsonb),
    (3, 'forge.graphite-billet', 'FORGE', 'material.graphite_billet', 1, '{}'::jsonb);

INSERT INTO content_recipe_inputs (
    policy_version, recipe_key, sequence, content_key, quantity
)
VALUES
    (3, 'forge.netherite-billet', 1, 'resource.netherite_scrap', 4),
    (3, 'forge.netherite-billet', 2, 'resource.ingot.gold', 4),
    (3, 'forge.graphite-precursor', 1, 'resource.ingot.titanium', 1),
    (3, 'forge.graphite-precursor', 2, 'resource.ingot.tungsten', 1),
    (3, 'forge.graphite-precursor', 3, 'resource.gem.onyx', 2),
    (3, 'forge.graphite-precursor', 4, 'resource.gem.diamond', 2),
    (3, 'forge.graphite-precursor', 5, 'resource.coal', 16),
    (3, 'forge.graphite-layer', 1, 'material.graphitic_precursor', 1),
    (3, 'forge.graphite-billet', 1, 'material.graphite_layer', 20);

UPDATE active_content_registry
   SET version = 3,
       activated_at = now()
 WHERE singleton = TRUE;

COMMIT;
