BEGIN;

ALTER TABLE pending_asset_deliveries
    ADD COLUMN item_instance_id UUID UNIQUE REFERENCES item_instances(id),
    ADD CONSTRAINT pending_asset_deliveries_unique_instance_shape CHECK (
        item_instance_id IS NULL
        OR (quantity = 1 AND desired_location = 'CATCH_BAG')
    );

INSERT INTO item_definitions (
    key, category, stackable, active, definition_version, rarity, stack_limit, unit_weight_grams, data
)
VALUES
    ('fishing.fish.bluegill', 'FISH', FALSE, TRUE, 1, 'COMMON', NULL, NULL, '{"species":"BLUEGILL"}'::jsonb),
    ('fishing.fish.carp', 'FISH', FALSE, TRUE, 1, 'COMMON', NULL, NULL, '{"species":"CARP"}'::jsonb),
    ('fishing.fish.catfish', 'FISH', FALSE, TRUE, 1, 'UNCOMMON', NULL, NULL, '{"species":"CATFISH"}'::jsonb),
    ('fishing.fish.koi', 'FISH', FALSE, TRUE, 1, 'RARE', NULL, NULL, '{"species":"KOI"}'::jsonb),
    ('fishing.fish.trout', 'FISH', FALSE, TRUE, 1, 'COMMON', NULL, NULL, '{"species":"TROUT"}'::jsonb),
    ('fishing.fish.salmon', 'FISH', FALSE, TRUE, 1, 'UNCOMMON', NULL, NULL, '{"species":"SALMON"}'::jsonb),
    ('fishing.fish.pike', 'FISH', FALSE, TRUE, 1, 'RARE', NULL, NULL, '{"species":"PIKE"}'::jsonb),
    ('fishing.fish.sturgeon', 'FISH', FALSE, TRUE, 1, 'EPIC', NULL, NULL, '{"species":"STURGEON"}'::jsonb),
    ('fishing.fish.bass', 'FISH', FALSE, TRUE, 1, 'COMMON', NULL, NULL, '{"species":"BASS"}'::jsonb),
    ('fishing.fish.mackerel', 'FISH', FALSE, TRUE, 1, 'COMMON', NULL, NULL, '{"species":"MACKEREL"}'::jsonb),
    ('fishing.fish.snapper', 'FISH', FALSE, TRUE, 1, 'UNCOMMON', NULL, NULL, '{"species":"SNAPPER"}'::jsonb),
    ('fishing.fish.tuna', 'FISH', FALSE, TRUE, 1, 'RARE', NULL, NULL, '{"species":"TUNA"}'::jsonb),
    ('fishing.fish.pufferfish', 'FISH', FALSE, TRUE, 1, 'RARE', NULL, NULL, '{"species":"PUFFERFISH"}'::jsonb),
    ('fishing.fish.swordfish', 'FISH', FALSE, TRUE, 1, 'EPIC', NULL, NULL, '{"species":"SWORDFISH"}'::jsonb),
    ('fishing.fish.marlin', 'FISH', FALSE, TRUE, 1, 'EPIC', NULL, NULL, '{"species":"MARLIN"}'::jsonb),
    ('fishing.fish.giant_grouper', 'FISH', FALSE, TRUE, 1, 'EPIC', NULL, NULL, '{"species":"GIANT_GROUPER"}'::jsonb),
    ('fishing.fish.shark', 'FISH', FALSE, TRUE, 1, 'LEGENDARY', NULL, NULL, '{"species":"SHARK"}'::jsonb),
    ('fishing.fish.coelacanth', 'FISH', FALSE, TRUE, 1, 'LEGENDARY', NULL, NULL, '{"species":"COELACANTH"}'::jsonb),
    ('fishing.fish.anglerfish', 'FISH', FALSE, TRUE, 1, 'RARE', NULL, NULL, '{"species":"ANGLERFISH"}'::jsonb),
    ('fishing.fish.abyss_eel', 'FISH', FALSE, TRUE, 1, 'EPIC', NULL, NULL, '{"species":"ABYSS_EEL"}'::jsonb),
    ('fishing.fish.moonfish', 'FISH', FALSE, TRUE, 1, 'LEGENDARY', NULL, NULL, '{"species":"MOONFISH"}'::jsonb),
    ('fishing.fish.leviathan_fry', 'FISH', FALSE, TRUE, 1, 'MYTHIC', NULL, NULL, '{"species":"LEVIATHAN_FRY"}'::jsonb);

INSERT INTO item_definition_versions (
    key, version, category, stackable, rarity, stack_limit, unit_weight_grams, data
)
SELECT key, definition_version, category, stackable, rarity, stack_limit, unit_weight_grams, data
  FROM item_definitions
 WHERE category = 'FISH'
   AND key LIKE 'fishing.fish.%';

COMMIT;
