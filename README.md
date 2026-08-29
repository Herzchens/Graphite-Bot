# Graphite Bot

Graphite is a persistent Discord game backend implemented in Rust with PostgreSQL as the authoritative state store.

This repository is being built in the order defined by the Graphite master specification. The current implementation establishes the production foundation rather than pretending unfinished gameplay systems are live.

## Implemented foundation

- Rust workspace split into deterministic domain primitives, economy services, item/storage services, frozen content/price registry services, progression services, service-policy math, PostgreSQL persistence, and the Discord application.
- UUIDv7 player and operation identities.
- Versioned Terms of Service storage and explicit acceptance during registration.
- Global player identity keyed by a unique Discord snowflake.
- Idempotent operation records with immutable request hashes, persisted RNG roots, committed results, and transactional outbox events.
- Starter equipment creation and automatic equip on first registration.
- Non-negative Wallet/Bank/liability materialized balances.
- Immutable double-entry ledger schema with deferred balance enforcement.
- Bank deposit/withdraw settlement with FIFO holding-age lots and canonical integer withdrawal-fee bands.
- Automatic Bank interest with deterministic fixed-point remainder, 0.004%/day base return, and the canonical Rebirth bonus capped to the first 10,000,000 Money.
- Soft-freeze accrual and hard-freeze pause semantics for Bank interest.
- System-authored Bank-interest operations with immutable ledger provenance and transactional outbox delivery.
- Version-pinned ItemDefinition history plus unique ItemInstances and stack-commodity storage.
- Item Bag capacity/occupancy accounting from the 36-slot baseline and definition-specific stack caps.
- CatchBag weight accounting from the 1,000 kg baseline using integer grams internally.
- Capacity-safe pending asset delivery instead of silent loss when an Item Bag delivery cannot fit.
- Transaction-composable exact-version stack delivery with stable per-operation mutation keys, replay-safe receipts, multiple pending sub-deliveries per operation, and serialized capacity enforcement.
- Death-safe Tool Locker, equipped-slot consistency constraints, item inspection, and idempotent equip/unequip mutations.
- Immutable versioned content/NPC-price registry containing the frozen resource lattice, explicit NPC-liquidity separation, normal-Shop availability classes, stock-policy classes, canonical alloy recipes, and versioned ordinary-smelting recipes.
- Integer-only regression math for the frozen processed-metal appraisal formula using the canonical 1/8-Coal fuel basis.
- Canonical equipment base-appraisal policy with frozen TierAnchor/SlotFactor tables, exact `round100` half-up arithmetic, and definition-specific override precedence.
- Canonical embedded-enchant appraisal policy with frozen acquisition weights, Level I–X multipliers, exact book values, and 70% aggregate contribution without Market-price input.
- Canonical base Repair economic policy math using exact durability fractions, frozen tier ratios/material recipes, `round100` Money settlement, Gold Activity EXP cost, and floor-80% cancellation material refunds without floating-point arithmetic.
- Canonical Smelting preview math using exact half-smelt heat units for Coal/Wood Log, 20-second base unit time, partial-fuel previews, stop/cancel fuel accounting, and per-job 8-unit Activity EXP remainder math.
- Persistent UUIDv7 service-job identity plus immutable per-job stack reservation provenance, with deterministic reservation locking and the unsafe aggregate `JOB_RESERVATION` Item Stack path retired.
- Immutable tickless Smelting runtime snapshots that freeze effective unit timing/modifier provenance at Confirm and derive progress in O(1) from timestamps without per-second job writes.
- Canonical Account XP/Level and derived Activity Level math, fixed Account Level Money rewards, deterministic Rebirth utility curves, and idempotent Rebirth reset semantics.
- Transaction-composable Activity EXP settlement for already-effective integer grants, spends, and losses, with stable per-operation mutation keys, mandatory provenance, non-negative enforcement, and replay-safe receipts.
- HMAC identity fingerprints for the temporary post-deletion re-registration cooldown.
- Deterministic domain-separated ChaCha12 RNG primitives seeded from the operating-system CSPRNG.
- Global text prefixes (`g`, `graphite`) and Discord mention parsing for the currently active command subset.
- Slash and text invocation paths share the same application handlers.

## Active command subset

Only implemented commands are registered right now:

- `/help`
- `/tos`
- `/register`
- `/profile`
- `/balance`
- `/bank`
- `/transactions`
- `/itembag` (`ib`, `bag`)
- `/catchbag` (`cb`, `fb`)
- `/locker` (`lk`, `tools`)
- `/equipment` (`eq`, `gear`)
- `/equip`
- `/unequip`
- `/item`

`/bank` supports balance information plus Wallet↔Bank deposit/withdraw mutations. Bank interest accrues automatically and is refreshed before balance-sensitive command paths; a bounded background worker catches up accounts that are not actively issuing commands.

The storage slice exposes safe reads plus equipment movement. Generic future reward systems can use the operation-owned ItemService delivery path, while composite gameplay/service owners can settle exact-version stack sub-deliveries inside their own PostgreSQL transaction. Both paths preserve capacity safety by writing an authoritative pending-delivery row instead of silently dropping assets when Item Bag capacity is insufficient. `/discard` is intentionally not registered yet because the master specification names Trash Recovery but this slice does not invent missing recovery/expiry behavior. Storage-capacity purchases, fishing, mining, remaining economy mutations, live services, market/trade, clan, automation, minigames, and other systems remain unavailable until their implementation slice satisfies the master specification.

The frozen content/price registry is deliberately read-only infrastructure. Policy v2 preserves the full v1 price/content lattice and adds the fourteen canonical one-input ordinary-smelting mappings, including Bauxite → Aluminum Ingot and Ancient Debris → Netherite Scrap. It still does not activate `/shop`, NPC liquidation, or stock mutation. The specification defines per-definition stack caps but does not freeze numeric caps for the new resource definitions, so the repository does not guess those values or prematurely activate these catalog rows as stack ItemDefinitions.

The equipment-appraisal foundation is deliberately pure policy math. Ordinary equipment resolves `BaseEquipmentAppraisal = round100(TierAnchor × SlotFactor)` from the frozen nine-tier/seven-slot tables using checked integer/rational arithmetic, while an explicit ItemDefinition `base_appraisal` override takes exact precedence without additional rounding. Embedded enchants can be valued from an already-resolved acquisition class and resulting Level I–X using the frozen `60,000 × AcquisitionWeight × LevelMultiplier` table; the aggregate contribution is `round_half_up(0.70 × ΣBookAppraisal)`. Shadow Walker is represented by the mid-high class at its resulting level, while SoulBind is deliberately absent because it is not an enchant and contributes zero to its own appraisal. Starter Leather has no canonical TierAnchor in the active table, so it requires an explicit definition override rather than borrowing another material's anchor. This slice still does not add an ItemDefinition appraisal resolver, enchant-definition→appraisal-class resolver, creation-roll storage/precision, +N appraisal, or the final `EnhancedCanonicalAppraisal`; those remain separate prerequisites before Repair/Forge/SoulBind can consume end-to-end authoritative appraisal state.

The Repair foundation is deliberately pure policy math. Given an already-resolved structural `RecraftAppraisal`, tier, equipment slot, and durability state, it computes the frozen full-repair Money/material recipe and Gold Activity EXP sink using checked integer/rational arithmetic. It also exposes the frozen cancellation refund rule for already-eligible material units. `/repair` remains unavailable: this slice does not resolve ItemInstance appraisal, reserve or mutate equipment/assets, create/settle Repair jobs, apply Grinding/Mosaic, or invent the still-unspecified Repair-time formula. Starter Leather Armor is represented as repairable material-wise, but its active specification does not define a Leather `TierRepairRatio`, so the kernel rejects that Money preview rather than borrowing another tier's ratio.

The Smelting foundation now includes pure preview/policy math, transaction-composable per-job stack reservation ownership, an immutable tickless runtime snapshot, pure terminal consequence planning, and the exact-version pending-safe stack-delivery primitive needed by a future terminal settlement owner. Runtime start uses PostgreSQL's actual clock at attachment rather than transaction-start time; effective unit duration and modifier provenance are frozen, and completed units are derived by flooring elapsed time over the frozen unit duration. New runtime attachment requires the owning operation to remain PENDING and the account to remain ACTIVE, while exact committed replay remains available. The runtime deliberately does not invent a speed-bucket formula or assume content registry keys are ItemDefinition keys. `/smelt` and Confirm remain unavailable because production resource ItemDefinitions/stack caps, an authoritative content↔ItemDefinition bridge, recipe/output snapshotting, authoritative Hard Freeze overlap tracking, the owning atomic terminal settlement transaction, and the higher-level atomic Confirm flow are not complete.

The progression domain owns canonical Account XP, Activity EXP, derived levels, and Rebirth state. User-facing `/level`/`/activity`/`/rebirth` command wiring and live chat/Mine/Fish/monster/Quest source adapters are still intentionally unavailable until their qualification, risk, and gameplay slices exist. The Activity EXP transaction API accepts already-effective integer points; source-specific Rebirth/guild/clan/event/automation modifiers remain the responsibility of the owning source so they cannot be silently double-applied.

## Requirements

The production baseline is deliberately pinned to stable releases that were verified on 2026-08-28:

- Rust 1.98.0 with edition 2024.
- PostgreSQL 18.6 for CI and the recommended production baseline.
- A Discord application/bot token.
- Discord Message Content intent if text-prefix commands are enabled.

Direct Rust dependencies are pinned in the workspace manifest. Upgrade them only after formatting, Clippy, workspace tests, PostgreSQL integration tests, and relevant behavioral regressions pass.

## Local setup

```bash
cp .env.example .env
# edit .env
cargo run -p graphite-bot
```

The bot automatically runs checked-in SQL migrations on startup.

For faster slash-command propagation during development, set `GRAPHITE_DEV_GUILD_ID`. Omit it for global command registration.

## Verification

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

The CI workflow provisions PostgreSQL 18.6 and runs PostgreSQL integration coverage. CI is intentionally read-only: it reports formatting or test failures instead of committing fixes back to the PR branch.

## Architecture rule

PostgreSQL owns canonical player, asset, storage, equipment, content, price-policy, progression, balance, operation, interest, service-job/reservation/runtime, and outbox state. In-memory data may accelerate reads, but it is never the source of truth for ownership, pricing policy, progression, or Money. Read-only policy previews may be computed in pure Rust, but state-changing handlers must revalidate authoritative inputs, resolve a bounded mutation, and settle it atomically before Discord side effects are emitted.
