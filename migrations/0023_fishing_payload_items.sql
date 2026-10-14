BEGIN;

INSERT INTO item_definitions (
    key, category, stackable, active, definition_version, rarity, stack_limit, unit_weight_grams, data
)
VALUES
    ('fishing.junk.driftwood', 'FISHING_JUNK', TRUE, TRUE, 1, 'COMMON', 64, NULL, '{"fishing_payload_family":"JUNK"}'::jsonb),
    ('fishing.junk.rusted_can', 'FISHING_JUNK', TRUE, TRUE, 1, 'COMMON', 64, NULL, '{"fishing_payload_family":"JUNK"}'::jsonb),
    ('fishing.junk.torn_net', 'FISHING_JUNK', TRUE, TRUE, 1, 'COMMON', 64, NULL, '{"fishing_payload_family":"JUNK"}'::jsonb),
    ('fishing.junk.broken_hook', 'FISHING_JUNK', TRUE, TRUE, 1, 'COMMON', 64, NULL, '{"fishing_payload_family":"JUNK"}'::jsonb),

    ('fishing.material_bundle.common', 'FISHING_MATERIAL_BUNDLE', TRUE, TRUE, 1, 'COMMON', 16, NULL, '{"fishing_payload_family":"MATERIAL_BUNDLE","sealed":true}'::jsonb),
    ('fishing.material_bundle.refined', 'FISHING_MATERIAL_BUNDLE', TRUE, TRUE, 1, 'COMMON', 16, NULL, '{"fishing_payload_family":"MATERIAL_BUNDLE","sealed":true}'::jsonb),
    ('fishing.material_bundle.deep', 'FISHING_MATERIAL_BUNDLE', TRUE, TRUE, 1, 'COMMON', 16, NULL, '{"fishing_payload_family":"MATERIAL_BUNDLE","sealed":true}'::jsonb),

    ('fishing.container.weathered_crate', 'FISHING_CONTAINER', TRUE, TRUE, 1, 'COMMON', 8, NULL, '{"fishing_payload_family":"CRATE_OR_CHEST","sealed":true}'::jsonb),
    ('fishing.container.ironbound_chest', 'FISHING_CONTAINER', TRUE, TRUE, 1, 'COMMON', 8, NULL, '{"fishing_payload_family":"CRATE_OR_CHEST","sealed":true}'::jsonb),
    ('fishing.container.sunken_cache', 'FISHING_CONTAINER', TRUE, TRUE, 1, 'COMMON', 8, NULL, '{"fishing_payload_family":"CRATE_OR_CHEST","sealed":true}'::jsonb),

    ('fishing.fragment.enchant_catalyst', 'FISHING_FRAGMENT', TRUE, TRUE, 1, 'COMMON', 64, NULL, '{"fishing_payload_family":"ORB_OR_CATALYST"}'::jsonb),
    ('fishing.fragment.normal_slot_orb', 'FISHING_FRAGMENT', TRUE, TRUE, 1, 'COMMON', 64, NULL, '{"fishing_payload_family":"ORB_OR_CATALYST"}'::jsonb),
    ('fishing.fragment.special_slot_orb', 'FISHING_FRAGMENT', TRUE, TRUE, 1, 'COMMON', 64, NULL, '{"fishing_payload_family":"ORB_OR_CATALYST"}'::jsonb),

    ('fishing.bait.school', 'FISHING_BAIT', TRUE, TRUE, 1, 'COMMON', 64, NULL, '{"bait_kind":"SCHOOL"}'::jsonb),
    ('fishing.bait.quality', 'FISHING_BAIT', TRUE, TRUE, 1, 'COMMON', 64, NULL, '{"bait_kind":"QUALITY"}'::jsonb),
    ('fishing.bait.rare', 'FISHING_BAIT', TRUE, TRUE, 1, 'COMMON', 64, NULL, '{"bait_kind":"RARE"}'::jsonb),
    ('fishing.bait.treasure', 'FISHING_BAIT', TRUE, TRUE, 1, 'COMMON', 64, NULL, '{"bait_kind":"TREASURE"}'::jsonb),
    ('fishing.bait.sturdy', 'FISHING_BAIT', TRUE, TRUE, 1, 'COMMON', 64, NULL, '{"bait_kind":"STURDY"}'::jsonb),

    ('fishing.relic.ancient_coin', 'FISHING_RELIC', TRUE, TRUE, 1, 'COMMON', 16, NULL, '{"fishing_payload_family":"RELIC_OR_COLLECTIBLE"}'::jsonb),
    ('fishing.relic.sea_glass', 'FISHING_RELIC', TRUE, TRUE, 1, 'COMMON', 16, NULL, '{"fishing_payload_family":"RELIC_OR_COLLECTIBLE"}'::jsonb),
    ('fishing.relic.fossil_shell', 'FISHING_RELIC', TRUE, TRUE, 1, 'COMMON', 16, NULL, '{"fishing_payload_family":"RELIC_OR_COLLECTIBLE"}'::jsonb),
    ('fishing.relic.abyssal_idol', 'FISHING_RELIC', TRUE, TRUE, 1, 'COMMON', 16, NULL, '{"fishing_payload_family":"RELIC_OR_COLLECTIBLE"}'::jsonb);

INSERT INTO item_definition_versions (
    key, version, category, stackable, rarity, stack_limit, unit_weight_grams, data
)
SELECT
    key,
    definition_version,
    category,
    stackable,
    rarity,
    stack_limit,
    unit_weight_grams,
    data
FROM item_definitions
WHERE key IN (
    'fishing.junk.driftwood',
    'fishing.junk.rusted_can',
    'fishing.junk.torn_net',
    'fishing.junk.broken_hook',
    'fishing.material_bundle.common',
    'fishing.material_bundle.refined',
    'fishing.material_bundle.deep',
    'fishing.container.weathered_crate',
    'fishing.container.ironbound_chest',
    'fishing.container.sunken_cache',
    'fishing.fragment.enchant_catalyst',
    'fishing.fragment.normal_slot_orb',
    'fishing.fragment.special_slot_orb',
    'fishing.bait.school',
    'fishing.bait.quality',
    'fishing.bait.rare',
    'fishing.bait.treasure',
    'fishing.bait.sturdy',
    'fishing.relic.ancient_coin',
    'fishing.relic.sea_glass',
    'fishing.relic.fossil_shell',
    'fishing.relic.abyssal_idol'
);

COMMIT;
