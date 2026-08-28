use graphite_store::PgStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use thiserror::Error;

#[derive(Clone)]
pub struct ContentRegistryService {
    store: PgStore,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RegistryPolicy {
    pub version: i32,
    pub label: String,
    pub source_reference: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AppraisalMode {
    Fixed,
    DerivedInput,
}

impl AppraisalMode {
    fn from_database(value: String) -> Result<Self, RegistryError> {
        match value.as_str() {
            "FIXED" => Ok(Self::Fixed),
            "DERIVED_INPUT" => Ok(Self::DerivedInput),
            _ => Err(RegistryError::InvalidRegistryValue {
                field: "appraisal_mode",
                value,
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ShopStockPolicy {
    WideOrPerUser,
    WeeklyLimited,
    NotSold,
}

impl ShopStockPolicy {
    fn from_database(value: String) -> Result<Self, RegistryError> {
        match value.as_str() {
            "WIDE_OR_PER_USER" => Ok(Self::WideOrPerUser),
            "WEEKLY_LIMITED" => Ok(Self::WeeklyLimited),
            "NOT_SOLD" => Ok(Self::NotSold),
            _ => Err(RegistryError::InvalidRegistryValue {
                field: "shop_stock_policy",
                value,
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PriceEntry {
    pub policy_version: i32,
    pub content_key: String,
    pub display_name: String,
    pub content_kind: String,
    pub source_class: String,
    pub appraisal_mode: AppraisalMode,
    pub canonical_appraisal: Option<i64>,
    pub npc_buy_price: Option<i64>,
    pub npc_liquidation_allowed: bool,
    pub shop_sell_price: Option<i64>,
    pub normal_shop_allowed: bool,
    pub shop_stock_policy: ShopStockPolicy,
    pub shop_class: String,
    pub metadata: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecipeInput {
    pub content_key: String,
    pub quantity: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContentRecipe {
    pub policy_version: i32,
    pub recipe_key: String,
    pub recipe_kind: String,
    pub output_content_key: String,
    pub output_quantity: i64,
    pub metadata: Value,
    pub inputs: Vec<RecipeInput>,
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("invalid registry value for {field}: {value}")]
    InvalidRegistryValue { field: &'static str, value: String },
}

impl From<sqlx::Error> for RegistryError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum PriceMathError {
    #[error("appraisal inputs must be non-negative")]
    NegativeInput,
    #[error("appraisal arithmetic overflowed")]
    ArithmeticOverflow,
}

impl ContentRegistryService {
    #[must_use]
    pub fn new(store: PgStore) -> Self {
        Self { store }
    }

    pub async fn active_policy(&self) -> Result<RegistryPolicy, RegistryError> {
        let row = sqlx::query(
            r#"
            SELECT v.version, v.label, v.source_reference
              FROM active_content_registry a
              JOIN content_registry_versions v ON v.version = a.version
             WHERE a.singleton = TRUE
            "#,
        )
        .fetch_one(self.store.pool())
        .await?;

        Ok(RegistryPolicy {
            version: row.try_get("version")?,
            label: row.try_get("label")?,
            source_reference: row.try_get("source_reference")?,
        })
    }

    pub async fn price(&self, content_key: &str) -> Result<Option<PriceEntry>, RegistryError> {
        let row = sqlx::query(
            r#"
            SELECT c.policy_version,
                   c.content_key,
                   c.display_name,
                   c.content_kind,
                   c.source_class,
                   p.appraisal_mode,
                   p.canonical_appraisal,
                   p.npc_buy_price,
                   p.npc_liquidation_allowed,
                   p.shop_sell_price,
                   p.normal_shop_allowed,
                   p.shop_stock_policy,
                   p.shop_class,
                   c.metadata
              FROM active_content_registry a
              JOIN content_catalog_entries c ON c.policy_version = a.version
              JOIN npc_price_entries p
                ON p.policy_version = c.policy_version
               AND p.content_key = c.content_key
             WHERE a.singleton = TRUE
               AND c.content_key = $1
            "#,
        )
        .bind(content_key)
        .fetch_optional(self.store.pool())
        .await?;

        row.map(row_to_price_entry).transpose()
    }

    pub async fn all_prices(&self) -> Result<Vec<PriceEntry>, RegistryError> {
        let rows = sqlx::query(
            r#"
            SELECT c.policy_version,
                   c.content_key,
                   c.display_name,
                   c.content_kind,
                   c.source_class,
                   p.appraisal_mode,
                   p.canonical_appraisal,
                   p.npc_buy_price,
                   p.npc_liquidation_allowed,
                   p.shop_sell_price,
                   p.normal_shop_allowed,
                   p.shop_stock_policy,
                   p.shop_class,
                   c.metadata
              FROM active_content_registry a
              JOIN content_catalog_entries c ON c.policy_version = a.version
              JOIN npc_price_entries p
                ON p.policy_version = c.policy_version
               AND p.content_key = c.content_key
             WHERE a.singleton = TRUE
             ORDER BY c.content_key
            "#,
        )
        .fetch_all(self.store.pool())
        .await?;

        rows.into_iter().map(row_to_price_entry).collect()
    }

    pub async fn shop_catalog(&self) -> Result<Vec<PriceEntry>, RegistryError> {
        let rows = sqlx::query(
            r#"
            SELECT c.policy_version,
                   c.content_key,
                   c.display_name,
                   c.content_kind,
                   c.source_class,
                   p.appraisal_mode,
                   p.canonical_appraisal,
                   p.npc_buy_price,
                   p.npc_liquidation_allowed,
                   p.shop_sell_price,
                   p.normal_shop_allowed,
                   p.shop_stock_policy,
                   p.shop_class,
                   c.metadata
              FROM active_content_registry a
              JOIN content_catalog_entries c ON c.policy_version = a.version
              JOIN npc_price_entries p
                ON p.policy_version = c.policy_version
               AND p.content_key = c.content_key
             WHERE a.singleton = TRUE
               AND p.normal_shop_allowed = TRUE
             ORDER BY p.shop_sell_price, c.content_key
            "#,
        )
        .fetch_all(self.store.pool())
        .await?;

        rows.into_iter().map(row_to_price_entry).collect()
    }

    pub async fn recipe(&self, recipe_key: &str) -> Result<Option<ContentRecipe>, RegistryError> {
        let row = sqlx::query(
            r#"
            SELECT r.policy_version,
                   r.recipe_key,
                   r.recipe_kind,
                   r.output_content_key,
                   r.output_quantity,
                   r.metadata
              FROM active_content_registry a
              JOIN content_recipes r ON r.policy_version = a.version
             WHERE a.singleton = TRUE
               AND r.recipe_key = $1
            "#,
        )
        .bind(recipe_key)
        .fetch_optional(self.store.pool())
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };
        let policy_version: i32 = row.try_get("policy_version")?;
        let inputs = sqlx::query(
            r#"
            SELECT content_key, quantity
              FROM content_recipe_inputs
             WHERE policy_version = $1
               AND recipe_key = $2
             ORDER BY sequence
            "#,
        )
        .bind(policy_version)
        .bind(recipe_key)
        .fetch_all(self.store.pool())
        .await?
        .into_iter()
        .map(|input| {
            Ok(RecipeInput {
                content_key: input.try_get("content_key")?,
                quantity: input.try_get("quantity")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;

        Ok(Some(ContentRecipe {
            policy_version,
            recipe_key: row.try_get("recipe_key")?,
            recipe_kind: row.try_get("recipe_kind")?,
            output_content_key: row.try_get("output_content_key")?,
            output_quantity: row.try_get("output_quantity")?,
            metadata: row.try_get("metadata")?,
            inputs,
        }))
    }
}

pub fn processed_appraisal(raw_appraisal: i64, coal_appraisal: i64) -> Result<i64, PriceMathError> {
    if raw_appraisal < 0 || coal_appraisal < 0 {
        return Err(PriceMathError::NegativeInput);
    }

    let fuel_eighths = i128::from(raw_appraisal)
        .checked_mul(8)
        .and_then(|value| value.checked_add(i128::from(coal_appraisal)))
        .ok_or(PriceMathError::ArithmeticOverflow)?;
    let numerator = fuel_eighths
        .checked_mul(1_005)
        .ok_or(PriceMathError::ArithmeticOverflow)?;
    let rounded = numerator
        .checked_add(4_000)
        .ok_or(PriceMathError::ArithmeticOverflow)?
        / 8_000;
    i64::try_from(rounded).map_err(|_| PriceMathError::ArithmeticOverflow)
}

fn row_to_price_entry(row: sqlx::postgres::PgRow) -> Result<PriceEntry, RegistryError> {
    Ok(PriceEntry {
        policy_version: row.try_get("policy_version")?,
        content_key: row.try_get("content_key")?,
        display_name: row.try_get("display_name")?,
        content_kind: row.try_get("content_kind")?,
        source_class: row.try_get("source_class")?,
        appraisal_mode: AppraisalMode::from_database(row.try_get("appraisal_mode")?)?,
        canonical_appraisal: row.try_get("canonical_appraisal")?,
        npc_buy_price: row.try_get("npc_buy_price")?,
        npc_liquidation_allowed: row.try_get("npc_liquidation_allowed")?,
        shop_sell_price: row.try_get("shop_sell_price")?,
        normal_shop_allowed: row.try_get("normal_shop_allowed")?,
        shop_stock_policy: ShopStockPolicy::from_database(row.try_get("shop_stock_policy")?)?,
        shop_class: row.try_get("shop_class")?,
        metadata: row.try_get("metadata")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processed_appraisal_matches_frozen_metal_examples() {
        let coal = 65;
        for (raw, processed) in [
            (95, 104),
            (105, 114),
            (120, 129),
            (135, 144),
            (190, 199),
            (240, 249),
            (280, 290),
            (330, 340),
            (380, 390),
            (2_800, 2_822),
            (5_200, 5_234),
            (6_500, 6_541),
            (8_200, 8_249),
            (9_000, 9_053),
        ] {
            assert_eq!(processed_appraisal(raw, coal).unwrap(), processed);
        }
    }

    #[test]
    fn processed_appraisal_rejects_negative_inputs() {
        assert_eq!(
            processed_appraisal(-1, 65),
            Err(PriceMathError::NegativeInput)
        );
    }
}
