BEGIN;

CREATE TABLE content_registry_versions (
    version INTEGER PRIMARY KEY CHECK (version > 0),
    label TEXT NOT NULL,
    source_reference TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE active_content_registry (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    version INTEGER NOT NULL REFERENCES content_registry_versions(version),
    activated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE content_catalog_entries (
    policy_version INTEGER NOT NULL REFERENCES content_registry_versions(version),
    content_key TEXT NOT NULL,
    display_name TEXT NOT NULL,
    content_kind TEXT NOT NULL CHECK (content_kind IN (
        'BLOCK', 'RESOURCE', 'ORE', 'INGOT', 'GEM', 'MATERIAL', 'ALLOY'
    )),
    source_class TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    PRIMARY KEY (policy_version, content_key),
    CHECK (content_key ~ '^[a-z0-9]+([._-][a-z0-9]+)+$')
);

CREATE TABLE npc_price_entries (
    policy_version INTEGER NOT NULL,
    content_key TEXT NOT NULL,
    appraisal_mode TEXT NOT NULL CHECK (appraisal_mode IN ('FIXED', 'DERIVED_INPUT')),
    canonical_appraisal BIGINT,
    npc_buy_price BIGINT,
    npc_liquidation_allowed BOOLEAN NOT NULL,
    shop_sell_price BIGINT,
    normal_shop_allowed BOOLEAN NOT NULL,
    shop_stock_policy TEXT NOT NULL CHECK (shop_stock_policy IN (
        'WIDE_OR_PER_USER', 'WEEKLY_LIMITED', 'NOT_SOLD'
    )),
    shop_class TEXT NOT NULL,
    PRIMARY KEY (policy_version, content_key),
    FOREIGN KEY (policy_version, content_key)
        REFERENCES content_catalog_entries(policy_version, content_key),
    CHECK (
        (appraisal_mode = 'FIXED' AND canonical_appraisal IS NOT NULL AND canonical_appraisal > 0)
        OR (appraisal_mode = 'DERIVED_INPUT' AND canonical_appraisal IS NULL)
    ),
    CHECK (npc_buy_price IS NULL OR npc_buy_price > 0),
    CHECK (shop_sell_price IS NULL OR shop_sell_price > 0),
    CHECK (npc_liquidation_allowed = (npc_buy_price IS NOT NULL)),
    CHECK (normal_shop_allowed = (shop_sell_price IS NOT NULL)),
    CHECK ((shop_stock_policy = 'NOT_SOLD') = (shop_sell_price IS NULL)),
    CHECK (npc_buy_price IS NULL OR shop_sell_price IS NULL OR npc_buy_price < shop_sell_price)
);

CREATE TABLE content_recipes (
    policy_version INTEGER NOT NULL REFERENCES content_registry_versions(version),
    recipe_key TEXT NOT NULL,
    recipe_kind TEXT NOT NULL,
    output_content_key TEXT NOT NULL,
    output_quantity BIGINT NOT NULL CHECK (output_quantity > 0),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    PRIMARY KEY (policy_version, recipe_key),
    FOREIGN KEY (policy_version, output_content_key)
        REFERENCES content_catalog_entries(policy_version, content_key)
);

CREATE TABLE content_recipe_inputs (
    policy_version INTEGER NOT NULL,
    recipe_key TEXT NOT NULL,
    sequence SMALLINT NOT NULL CHECK (sequence > 0),
    content_key TEXT NOT NULL,
    quantity BIGINT NOT NULL CHECK (quantity > 0),
    PRIMARY KEY (policy_version, recipe_key, sequence),
    FOREIGN KEY (policy_version, recipe_key)
        REFERENCES content_recipes(policy_version, recipe_key),
    FOREIGN KEY (policy_version, content_key)
        REFERENCES content_catalog_entries(policy_version, content_key)
);

CREATE INDEX npc_price_entries_shop_idx
    ON npc_price_entries (policy_version, normal_shop_allowed, shop_sell_price, content_key);

CREATE OR REPLACE FUNCTION graphite_forbid_frozen_registry_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Graphite frozen content/price registry rows are immutable; create a newer policy version';
END;
$$;

CREATE TRIGGER content_registry_versions_immutable
BEFORE UPDATE OR DELETE ON content_registry_versions
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_frozen_registry_mutation();

CREATE TRIGGER content_catalog_entries_immutable
BEFORE UPDATE OR DELETE ON content_catalog_entries
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_frozen_registry_mutation();

CREATE TRIGGER npc_price_entries_immutable
BEFORE UPDATE OR DELETE ON npc_price_entries
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_frozen_registry_mutation();

CREATE TRIGGER content_recipes_immutable
BEFORE UPDATE OR DELETE ON content_recipes
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_frozen_registry_mutation();

CREATE TRIGGER content_recipe_inputs_immutable
BEFORE UPDATE OR DELETE ON content_recipe_inputs
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_frozen_registry_mutation();

INSERT INTO content_registry_versions (version, label, source_reference)
VALUES (
    1,
    'Graphite frozen NPC/content lattice v1',
    'Graphite Master Specification Appendix A fixed NPC price lattice and alloy rules'
);

INSERT INTO active_content_registry (singleton, version)
VALUES (TRUE, 1);

INSERT INTO content_catalog_entries (
    policy_version, content_key, display_name, content_kind, source_class, metadata
)
VALUES
    (1, 'resource.cobblestone', 'Cobblestone', 'BLOCK', 'COMMON', '{}'::jsonb),
    (1, 'resource.wood.log', 'Wood Log', 'RESOURCE', 'ESSENTIAL_COMMON', '{}'::jsonb),
    (1, 'resource.stone', 'Stone', 'BLOCK', 'COMMON', '{}'::jsonb),
    (1, 'resource.netherrack', 'Netherrack', 'BLOCK', 'COMMON', '{}'::jsonb),
    (1, 'resource.deepslate', 'Deepslate', 'BLOCK', 'COMMON', '{}'::jsonb),
    (1, 'resource.blackstone', 'Blackstone', 'BLOCK', 'COMMON', '{}'::jsonb),
    (1, 'resource.redstone', 'Redstone', 'RESOURCE', 'COMMON', '{}'::jsonb),
    (1, 'resource.coal', 'Coal', 'RESOURCE', 'ESSENTIAL_COMMON', '{}'::jsonb),
    (1, 'resource.leather', 'Leather', 'RESOURCE', 'ESSENTIAL_COMMON', '{}'::jsonb),
    (1, 'resource.lapis', 'Lapis', 'RESOURCE', 'COMMON', '{}'::jsonb),
    (1, 'resource.ore.tin', 'Tin Ore', 'ORE', 'COMMON', jsonb_build_object('role', 'Bronze and low/mid mechanical components')),
    (1, 'resource.ingot.tin', 'Tin Ingot', 'INGOT', 'COMMON', jsonb_build_object('role', 'Bronze and low/mid mechanical components')),
    (1, 'resource.ore.copper', 'Copper Ore', 'ORE', 'COMMON', '{}'::jsonb),
    (1, 'resource.ingot.copper', 'Copper Ingot', 'INGOT', 'COMMON', '{}'::jsonb),
    (1, 'resource.ore.zinc', 'Zinc Ore', 'ORE', 'COMMON', jsonb_build_object('role', 'Brass and precision/Automation components')),
    (1, 'resource.ingot.zinc', 'Zinc Ingot', 'INGOT', 'COMMON', jsonb_build_object('role', 'Brass and precision/Automation components')),
    (1, 'resource.bauxite', 'Bauxite', 'ORE', 'COMMON', jsonb_build_object('role', 'Aluminum source for lightweight Rod components, tanks and Automation frames')),
    (1, 'resource.ingot.aluminum', 'Aluminum Ingot', 'INGOT', 'COMMON', jsonb_build_object('role', 'Lightweight Rod components, tanks and Automation frames')),
    (1, 'resource.ore.iron', 'Iron Ore', 'ORE', 'COMMON', '{}'::jsonb),
    (1, 'resource.ingot.iron', 'Iron Ingot', 'INGOT', 'COMMON', '{}'::jsonb),
    (1, 'resource.ore.lead', 'Lead Ore', 'ORE', 'LIMITED', jsonb_build_object('role', 'Heavy reinforcement and Guardian/Reinforce recipes; non-radioactive')),
    (1, 'resource.ingot.lead', 'Lead Ingot', 'INGOT', 'LIMITED', jsonb_build_object('role', 'Heavy reinforcement and Guardian/Reinforce recipes; non-radioactive')),
    (1, 'resource.quartz.nether', 'Nether Quartz', 'RESOURCE', 'LIMITED', '{}'::jsonb),
    (1, 'resource.ore.silver', 'Silver Ore', 'ORE', 'LIMITED', jsonb_build_object('role', 'Enchant/catalyst and Angel/Smite-oriented recipes')),
    (1, 'resource.ingot.silver', 'Silver Ingot', 'INGOT', 'LIMITED', jsonb_build_object('role', 'Enchant/catalyst and Angel/Smite-oriented recipes')),
    (1, 'resource.ore.nickel', 'Nickel Ore', 'ORE', 'LIMITED', jsonb_build_object('role', 'Invar, durability and Reinforce/Grinding components')),
    (1, 'resource.ingot.nickel', 'Nickel Ingot', 'INGOT', 'LIMITED', jsonb_build_object('role', 'Invar, durability and Reinforce/Grinding components')),
    (1, 'resource.ore.gold', 'Gold Ore', 'ORE', 'LIMITED', '{}'::jsonb),
    (1, 'resource.ingot.gold', 'Gold Ingot', 'INGOT', 'LIMITED', '{}'::jsonb),
    (1, 'resource.gem.amethyst', 'Amethyst', 'GEM', 'MINING_ONLY', jsonb_build_object('role', 'Enchant books, catalysts and Slot-Orb recipes')),
    (1, 'resource.gem.jade', 'Jade', 'GEM', 'MINING_ONLY', jsonb_build_object('role', 'Repair, Mending and SoulGrind-oriented components')),
    (1, 'resource.gem.emerald', 'Emerald', 'GEM', 'RESERVED', jsonb_build_object('note', 'No active v1 source; future mountain source only')),
    (1, 'resource.gem.topaz', 'Topaz', 'GEM', 'MINING_ONLY', jsonb_build_object('role', 'Luck/Treasure-oriented components')),
    (1, 'resource.gem.ruby', 'Ruby', 'GEM', 'MINING_ONLY', jsonb_build_object('role', 'Offense, Crit and Fire/Blood-oriented components')),
    (1, 'resource.gem.sapphire', 'Sapphire', 'GEM', 'MINING_ONLY', jsonb_build_object('role', 'Defense, Protection, Freezing and Strengthen components')),
    (1, 'resource.gem.diamond', 'Diamond', 'GEM', 'MINING_ONLY', '{}'::jsonb),
    (1, 'resource.gem.onyx', 'Onyx', 'GEM', 'DEEP_MINING_ONLY', jsonb_build_object('role', 'SoulBind and Guardian/Nine-Life protection components')),
    (1, 'resource.ore.cobalt', 'Cobalt Ore', 'ORE', 'NETHER_ONLY', jsonb_build_object('role', 'High-strength Nether alloy component')),
    (1, 'resource.ingot.cobalt', 'Cobalt Ingot', 'INGOT', 'NOT_SHOP_SOLD', jsonb_build_object('role', 'High-strength Nether alloy component')),
    (1, 'resource.obsidian', 'Obsidian', 'RESOURCE', 'OBSIDIAN_CHASM_ONLY', '{}'::jsonb),
    (1, 'resource.ore.titanium', 'Titanium Ore', 'ORE', 'DEEP_NETHER_ONLY', jsonb_build_object('role', 'Endgame structural/tool component')),
    (1, 'resource.ingot.titanium', 'Titanium Ingot', 'INGOT', 'NOT_SHOP_SOLD', jsonb_build_object('role', 'Endgame structural/tool component')),
    (1, 'resource.ore.tungsten', 'Tungsten Ore', 'ORE', 'DEEP_NETHER_ONLY', jsonb_build_object('role', 'Extreme durability/armor reinforcement')),
    (1, 'resource.ingot.tungsten', 'Tungsten Ingot', 'INGOT', 'NOT_SHOP_SOLD', jsonb_build_object('role', 'Extreme durability/armor reinforcement')),
    (1, 'resource.ancient_debris', 'Ancient Debris', 'ORE', 'NETHER_ONLY', '{}'::jsonb),
    (1, 'resource.netherite_scrap', 'Netherite Scrap', 'MATERIAL', 'NOT_SHOP_SOLD', '{}'::jsonb),
    (1, 'material.netherite_billet', 'Netherite Billet', 'MATERIAL', 'FORGE_ONLY', '{}'::jsonb),
    (1, 'material.graphitic_precursor', 'Graphitic Precursor', 'MATERIAL', 'FORGE_ONLY', '{}'::jsonb),
    (1, 'material.graphite_layer', 'Graphite Layer', 'MATERIAL', 'FORGE_ONLY', jsonb_build_object('note', 'Expected-cost appraisal')),
    (1, 'material.graphite_billet', 'Graphite Billet', 'MATERIAL', 'FORGE_ONLY', jsonb_build_object('note', '20 Graphite Layers')),
    (1, 'resource.ore.platinum', 'Platinum Ore', 'ORE', 'DEEP_NETHER_ONLY', jsonb_build_object('role', 'High-tier Special Slot Orb / catalyst material')),
    (1, 'resource.ingot.platinum', 'Platinum Ingot', 'INGOT', 'NOT_SHOP_SOLD', jsonb_build_object('role', 'High-tier Special Slot Orb / catalyst material')),
    (1, 'resource.gem.blood_diamond', 'Blood Diamond', 'GEM', 'ULTRA_RARE_DIAMOND_MUTATION', '{}'::jsonb),
    (1, 'alloy.ingot.bronze', 'Bronze Ingot', 'ALLOY', 'ALLOY_FORGE', '{}'::jsonb),
    (1, 'alloy.ingot.brass', 'Brass Ingot', 'ALLOY', 'ALLOY_FORGE', '{}'::jsonb),
    (1, 'alloy.ingot.invar', 'Invar Ingot', 'ALLOY', 'ALLOY_FORGE', '{}'::jsonb),
    (1, 'alloy.ingot.electrum', 'Electrum Ingot', 'ALLOY', 'ALLOY_FORGE', '{}'::jsonb);

INSERT INTO npc_price_entries (
    policy_version, content_key, appraisal_mode, canonical_appraisal,
    npc_buy_price, npc_liquidation_allowed, shop_sell_price,
    normal_shop_allowed, shop_stock_policy, shop_class
)
VALUES
    (1, 'resource.cobblestone', 'FIXED', 18, 18, TRUE, 50, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.wood.log', 'FIXED', 24, 24, TRUE, 65, TRUE, 'WIDE_OR_PER_USER', 'ESSENTIAL_COMMON'),
    (1, 'resource.stone', 'FIXED', 28, 28, TRUE, 75, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.netherrack', 'FIXED', 32, 32, TRUE, 85, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.deepslate', 'FIXED', 42, 42, TRUE, 115, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.blackstone', 'FIXED', 45, 45, TRUE, 130, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.redstone', 'FIXED', 55, 55, TRUE, 160, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.coal', 'FIXED', 65, 65, TRUE, 175, TRUE, 'WIDE_OR_PER_USER', 'ESSENTIAL_COMMON'),
    (1, 'resource.leather', 'FIXED', 70, 70, TRUE, 190, TRUE, 'WIDE_OR_PER_USER', 'ESSENTIAL_COMMON'),
    (1, 'resource.lapis', 'FIXED', 80, 80, TRUE, 230, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.ore.tin', 'FIXED', 95, 95, TRUE, 260, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.ingot.tin', 'FIXED', 104, 104, TRUE, 340, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.ore.copper', 'FIXED', 105, 105, TRUE, 290, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.ingot.copper', 'FIXED', 114, 114, TRUE, 360, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.ore.zinc', 'FIXED', 120, 120, TRUE, 330, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.ingot.zinc', 'FIXED', 129, 129, TRUE, 420, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.bauxite', 'FIXED', 135, 135, TRUE, 370, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.ingot.aluminum', 'FIXED', 144, 144, TRUE, 470, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.ore.iron', 'FIXED', 190, 190, TRUE, 550, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.ingot.iron', 'FIXED', 199, 199, TRUE, 680, TRUE, 'WIDE_OR_PER_USER', 'COMMON'),
    (1, 'resource.ore.lead', 'FIXED', 240, 240, TRUE, 780, TRUE, 'WEEKLY_LIMITED', 'LIMITED'),
    (1, 'resource.ingot.lead', 'FIXED', 249, 249, TRUE, 900, TRUE, 'WEEKLY_LIMITED', 'LIMITED'),
    (1, 'resource.quartz.nether', 'FIXED', 260, 260, TRUE, 780, TRUE, 'WEEKLY_LIMITED', 'LIMITED'),
    (1, 'resource.ore.silver', 'FIXED', 280, 280, TRUE, 900, TRUE, 'WEEKLY_LIMITED', 'LIMITED'),
    (1, 'resource.ingot.silver', 'FIXED', 290, 290, TRUE, 1050, TRUE, 'WEEKLY_LIMITED', 'LIMITED'),
    (1, 'resource.ore.nickel', 'FIXED', 330, 330, TRUE, 1100, TRUE, 'WEEKLY_LIMITED', 'LIMITED'),
    (1, 'resource.ingot.nickel', 'FIXED', 340, 340, TRUE, 1250, TRUE, 'WEEKLY_LIMITED', 'LIMITED'),
    (1, 'resource.ore.gold', 'FIXED', 380, 380, TRUE, 1200, TRUE, 'WEEKLY_LIMITED', 'LIMITED'),
    (1, 'resource.ingot.gold', 'FIXED', 390, 390, TRUE, 1450, TRUE, 'WEEKLY_LIMITED', 'LIMITED'),
    (1, 'resource.gem.amethyst', 'FIXED', 450, 450, TRUE, NULL, FALSE, 'NOT_SOLD', 'MINING_ONLY'),
    (1, 'resource.gem.jade', 'FIXED', 650, 650, TRUE, NULL, FALSE, 'NOT_SOLD', 'MINING_ONLY'),
    (1, 'resource.gem.emerald', 'FIXED', 850, 850, TRUE, NULL, FALSE, 'NOT_SOLD', 'RESERVED'),
    (1, 'resource.gem.topaz', 'FIXED', 900, 900, TRUE, NULL, FALSE, 'NOT_SOLD', 'MINING_ONLY'),
    (1, 'resource.gem.ruby', 'FIXED', 1450, 1450, TRUE, NULL, FALSE, 'NOT_SOLD', 'MINING_ONLY'),
    (1, 'resource.gem.sapphire', 'FIXED', 1550, 1550, TRUE, NULL, FALSE, 'NOT_SOLD', 'MINING_ONLY'),
    (1, 'resource.gem.diamond', 'FIXED', 1700, 1700, TRUE, NULL, FALSE, 'NOT_SOLD', 'MINING_ONLY'),
    (1, 'resource.gem.onyx', 'FIXED', 2600, 2600, TRUE, NULL, FALSE, 'NOT_SOLD', 'DEEP_MINING_ONLY'),
    (1, 'resource.ore.cobalt', 'FIXED', 2800, 2800, TRUE, NULL, FALSE, 'NOT_SOLD', 'NETHER_ONLY'),
    (1, 'resource.ingot.cobalt', 'FIXED', 2822, 2822, TRUE, NULL, FALSE, 'NOT_SOLD', 'NOT_SHOP_SOLD'),
    (1, 'resource.obsidian', 'FIXED', 3400, 3400, TRUE, NULL, FALSE, 'NOT_SOLD', 'OBSIDIAN_CHASM_ONLY'),
    (1, 'resource.ore.titanium', 'FIXED', 5200, 5200, TRUE, NULL, FALSE, 'NOT_SOLD', 'DEEP_NETHER_ONLY'),
    (1, 'resource.ingot.titanium', 'FIXED', 5234, 5234, TRUE, NULL, FALSE, 'NOT_SOLD', 'NOT_SHOP_SOLD'),
    (1, 'resource.ore.tungsten', 'FIXED', 6500, 6500, TRUE, NULL, FALSE, 'NOT_SOLD', 'DEEP_NETHER_ONLY'),
    (1, 'resource.ingot.tungsten', 'FIXED', 6541, 6541, TRUE, NULL, FALSE, 'NOT_SOLD', 'NOT_SHOP_SOLD'),
    (1, 'resource.ancient_debris', 'FIXED', 8200, 8200, TRUE, NULL, FALSE, 'NOT_SOLD', 'NETHER_ONLY'),
    (1, 'resource.netherite_scrap', 'FIXED', 8249, 8249, TRUE, NULL, FALSE, 'NOT_SOLD', 'NOT_SHOP_SOLD'),
    (1, 'material.netherite_billet', 'FIXED', 34556, NULL, FALSE, NULL, FALSE, 'NOT_SOLD', 'FORGE_ONLY_NON_NPC_SELLABLE'),
    (1, 'material.graphitic_precursor', 'FIXED', 36415, NULL, FALSE, NULL, FALSE, 'NOT_SOLD', 'FORGE_ONLY_NON_NPC_SELLABLE'),
    (1, 'material.graphite_layer', 'FIXED', 103538, NULL, FALSE, NULL, FALSE, 'NOT_SOLD', 'EXPECTED_COST_NON_NPC_SELLABLE'),
    (1, 'material.graphite_billet', 'FIXED', 2570750, NULL, FALSE, NULL, FALSE, 'NOT_SOLD', 'TWENTY_LAYERS_NON_NPC_SELLABLE'),
    (1, 'resource.ore.platinum', 'FIXED', 9000, 9000, TRUE, NULL, FALSE, 'NOT_SOLD', 'DEEP_NETHER_ONLY'),
    (1, 'resource.ingot.platinum', 'FIXED', 9053, 9053, TRUE, NULL, FALSE, 'NOT_SOLD', 'NOT_SHOP_SOLD'),
    (1, 'resource.gem.blood_diamond', 'FIXED', 20000, 20000, TRUE, NULL, FALSE, 'NOT_SOLD', 'ULTRA_RARE_DIAMOND_MUTATION'),
    (1, 'alloy.ingot.bronze', 'DERIVED_INPUT', NULL, NULL, FALSE, 450, TRUE, 'WEEKLY_LIMITED', 'LIMITED'),
    (1, 'alloy.ingot.brass', 'DERIVED_INPUT', NULL, NULL, FALSE, 480, TRUE, 'WEEKLY_LIMITED', 'LIMITED'),
    (1, 'alloy.ingot.invar', 'DERIVED_INPUT', NULL, NULL, FALSE, 1100, TRUE, 'WEEKLY_LIMITED', 'LIMITED'),
    (1, 'alloy.ingot.electrum', 'DERIVED_INPUT', NULL, NULL, FALSE, 1500, TRUE, 'WEEKLY_LIMITED', 'LIMITED');

INSERT INTO content_recipes (
    policy_version, recipe_key, recipe_kind, output_content_key, output_quantity, metadata
)
VALUES
    (1, 'alloy.bronze', 'ALLOY', 'alloy.ingot.bronze', 4, jsonb_build_object('npc_liquidation', false)),
    (1, 'alloy.brass', 'ALLOY', 'alloy.ingot.brass', 3, jsonb_build_object('npc_liquidation', false)),
    (1, 'alloy.invar', 'ALLOY', 'alloy.ingot.invar', 3, jsonb_build_object('npc_liquidation', false)),
    (1, 'alloy.electrum', 'ALLOY', 'alloy.ingot.electrum', 2, jsonb_build_object('npc_liquidation', false));

INSERT INTO content_recipe_inputs (policy_version, recipe_key, sequence, content_key, quantity)
VALUES
    (1, 'alloy.bronze', 1, 'resource.ingot.copper', 3),
    (1, 'alloy.bronze', 2, 'resource.ingot.tin', 1),
    (1, 'alloy.brass', 1, 'resource.ingot.copper', 2),
    (1, 'alloy.brass', 2, 'resource.ingot.zinc', 1),
    (1, 'alloy.invar', 1, 'resource.ingot.iron', 2),
    (1, 'alloy.invar', 2, 'resource.ingot.nickel', 1),
    (1, 'alloy.electrum', 1, 'resource.ingot.gold', 1),
    (1, 'alloy.electrum', 2, 'resource.ingot.silver', 1);

COMMIT;
