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
- Immutable versioned content/NPC-price registry containing the frozen resource lattice, explicit NPC-liquidity separation, normal-Shop availability classes, stock-policy classes, canonical alloy recipes, versioned ordinary-smelting recipes, and versioned advanced Forge stack-recipe mappings.
- Integer-only regression math for the frozen processed-metal appraisal formula using the canonical 1/8-Coal fuel basis.
- Canonical equipment base-appraisal policy with frozen TierAnchor/SlotFactor tables, exact `round100` half-up arithmetic, and definition-specific override precedence.
- Canonical embedded-enchant appraisal policy with frozen acquisition weights, Level I–X multipliers, exact book values, and 70% aggregate contribution without Market-price input.
- Canonical +N appraisal-factor policy with exact rational `MainMult(N)` / `UpgradeFactor(N)` math and no intermediate rounding before final enhanced-appraisal composition.
- Canonical Slot Orb policy with the frozen five unlock thresholds/success fractions/base prices, player-paid application-fee ceiling, and failure/modifier semantics without live RNG or item mutation.
- Canonical SoulBind policy with frozen ordinary-Netherite/Graphite eligibility and packages, path-independent 60% positive-appraisal charging, 20% unbind fee, and seven-day rebind cooldown without live binding mutation.
- Canonical advanced Forge policy for Netherite/Graphite stack processing and Obsidian→Netherite→Graphite same-ItemInstance promotions, including exact Money/AEXP/time/success semantics and checked floor-ratio durability projection.
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

The frozen content/price registry is deliberately read-only infrastructure. Policy v2 preserves the full v1 price/content lattice and adds the fourteen canonical one-input ordinary-smelting mappings, including Bauxite → Aluminum Ingot and Ancient Debris → Netherite Scrap. Policy v3 preserves the complete v2 lattice and adds only the four canonical stack-output advanced Forge mappings for Netherite Billet, Graphitic Precursor, Graphite Layer, and Graphite Billet. Money, Activity EXP, duration, success/failure, cancellation, RNG snapshotting, and same-ItemInstance promotion semantics remain in the Services policy layer rather than being duplicated into recipe JSON. The registry still does not activate `/shop`, NPC liquidation, stock mutation, or `/forge`. The specification defines per-definition stack caps but does not freeze numeric caps for the new resource definitions, so the repository does not guess those values or prematurely activate these catalog rows as stack ItemDefinitions.

The equipment-appraisal foundation is deliberately pure policy math. Ordinary equipment resolves `BaseEquipmentAppraisal = round100(TierAnchor × SlotFactor)` from the frozen nine-tier/seven-slot tables using checked integer/rational arithmetic, while an explicit ItemDefinition `base_appraisal` override takes exact precedence without additional rounding. Embedded enchants can be valued from an already-resolved acquisition class and resulting Level I–X using the frozen `60,000 × AcquisitionWeight × LevelMultiplier` table; the aggregate contribution is `round_half_up(0.70 × ΣBookAppraisal)`. +N power/appraisal contribution is represented exactly from `MainMult(N) = 1 + 0.03238N + 0.000952N²` and `UpgradeFactor(N) = 1 + 0.55 × (MainMult(N) - 1)` using checked rational intermediates; applying that factor to base appraisal deliberately stays unrounded so the final composition can multiply by Creation Roll first. Shadow Walker is represented by the mid-high class at its resulting level, while SoulBind is deliberately absent because it is not an enchant and contributes zero to its own appraisal. Starter Leather has no canonical TierAnchor in the active table, so it requires an explicit definition override rather than borrowing another material's anchor. This foundation still does not add an ItemDefinition appraisal resolver, enchant-definition→appraisal-class resolver, creation-roll storage/precision, or the final `EnhancedCanonicalAppraisal`; the full +N attempt-cost path also remains pending because the specification does not freeze a deterministic evaluation algorithm for `round10(20 × N^1.55)`.

The Slot Orb foundation is deliberately pure policy/preview math. Normal/class slots start at four and expose #5 at +5 and #6 at +10; Special/universal slots start at three and expose #4 at +7, #5 at +12, and #6 at +15. The five frozen success chances are represented as exact reduced fractions, and the application fee uses checked integer `ceil(current EnhancedCanonicalAppraisal × percentage)` as required for player-paid percentage fees. +N only makes an Orb attempt eligible and never grants the slot for free. Failure policy records that the Orb plus application fee are consumed while equipment/slot state remains unchanged, and Sparkling/Mosaic are explicitly excluded. No `/enchant` or Slot Orb mutation is live: the caller must still resolve the current enhanced appraisal, lock/revalidate ItemInstance slot state and Orb ownership, draw deterministic RNG, and settle Orb/Money/slot mutation atomically.

The SoulBind foundation is deliberately pure policy/preview math. Only ordinary Netherite and Graphite equipment is eligible and the account must have Rebirth ≥1. The binding preview requires an already-resolved ordinary-equipment classification and current `EnhancedCanonicalAppraisal`; a future stateful owner must derive and revalidate both from the authoritative versioned ItemDefinition/ItemInstance rather than trust Discord input or cached state. The frozen fixed packages are one SoulBind Rune + 20 Onyx + 8 Platinum Ingots + 2 Netherite Billets + 250,000 Money + 25,000 AEXP for Netherite, and one Rune + 32 Onyx + 12 Platinum Ingots + 2 Graphite Layers + 500,000 Money + 50,000 AEXP for Graphite. Initial protection liability is `ceil(60% × current EnhancedCanonicalAppraisal)`. For one positive appraisal mutation from `previous` to `new`, the top-up is `ceil(60% × new) - ceil(60% × previous)` rather than an independently rounded raw-delta fee; these differences telescope over monotonic increases and preserve the explicit `bind-early + top-ups == bind-late` invariant. Equal or decreasing appraisal produces no top-up and no refund. The pure kernel deliberately does not invent historical-high-watermark semantics for later decrease/rebound sequences. Unbind charges `ceil(20% × current EnhancedCanonicalAppraisal)`, refunds no binding resources, requires Protected/Favorite to be cleared, and applies a seven-day item rebind cooldown. SoulBind is not live yet: the repository still lacks an authoritative SoulBind Rune ItemDefinition/stack cap, a typed ordinary-equipment resolver, persisted SoulBound state, atomic Rune/material/Money/AEXP settlement, an unbind transaction, a true-death protection hook, and a final authoritative enhanced-appraisal resolver.

The advanced Forge foundation is also deliberately non-mutating policy. Versioned registry rows freeze stack input/output mapping for Netherite Billet and the three Graphite processing materials. Services freeze their Money/AEXP costs, durations, exact success fractions, and post-Confirm cancellation/failure rules; Graphite Layer is represented exactly as `2/5` success without choosing an RNG-to-percent mapping. Obsidian→Netherite and Netherite→Graphite are represented as same-ItemInstance promotions with frozen component/cost/time policy, and current durability projects as `floor(old_current × new_max / old_max)` so a Broken item stays Broken. `/forge` remains unavailable: this foundation does not reserve inputs or Money/AEXP, draw or snapshot operational RNG, create/settle Forge jobs, mutate an ItemInstance tier, validate enchant/slot compatibility, resolve production ItemDefinitions/stack caps, or integrate the already-defined SoulBind positive-appraisal top-up with authoritative previous/new appraisal and atomic settlement for a bound Netherite→Graphite promotion.

The Repair foundation is deliberately pure policy math. Given an already-resolved structural `RecraftAppraisal`, tier, equipment slot, and durability state, it computes the frozen full-repair Money/material recipe and Gold Activity EXP sink using checked integer/rational arithmetic. It also exposes the frozen cancellation refund rule for already-eligible material units. `/repair` remains unavailable: this slice does not resolve ItemInstance appraisal, reserve or mutate equipment/assets, create/settle Repair jobs, apply Grinding/Mosaic, or invent the still-unspecified Repair-time formula. Starter Leather Armor is represented as repairable material-wise, but its active specification does not define a Leather `TierRepairRatio`, so the kernel rejects that Money preview rather than borrowing another tier's ratio.

The Smelting foundation now includes pure preview/policy math, transaction-composable per-job stack reservation ownership, an immutable tickless runtime snapshot, pure terminal consequence planning, and the exact-version pending-safe stack-delivery primitive needed by a future terminal settlement owner. Runtime start uses PostgreSQL's actual clock at attachment rather than transaction-start time; effective unit duration and modifier provenance are frozen, and completed units are derived by flooring elapsed time over the frozen unit duration. New runtime attachment requires the owning operation to remain PENDING and the account to remain ACTIVE, while exact committed replay remains available. The runtime deliberately does not invent a speed-bucket formula or assume content registry keys are ItemDefinition keys. `/smelt` and Confirm remain unavailable because production resource ItemDefinitions/stack caps, an authoritative content↔ItemDefinition bridge, recipe/output snapshotting, authoritative Hard Freeze overlap tracking, the owning atomic terminal settlement transaction, and the higher-level atomic Confirm flow are not complete.

The progression domain owns canonical Account XP, Activity EXP, derived levels, and Rebirth state. User-facing `/level`/`/activity`/`/rebirth` command wiring and live chat/Mine/Fish/monster/Quest source adapters are still intentionally unavailable until their qualification, risk, and gameplay slices exist. The Activity EXP transaction API accepts already-effective integer points; source-specific Rebirth/guild/clan/event/automation modifiers remain the responsibility of the owning source so they cannot be silently double-applied.

## Requirements

The production baseline is deliberately pinned to stable releases that were re-checked on 2026-08-29:

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
