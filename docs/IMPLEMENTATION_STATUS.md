# Implementation status

This file tracks implementation against the build-order recommendation in the Graphite master specification.

| Phase | Scope | Status |
| --- | --- | --- |
| 1 | Identity / ToS / global player / PostgreSQL foundations | Implemented foundation |
| 2 | Ledger / operations / idempotency / outbox | Bank deposit/withdraw and automatic interest-accrual slices implemented with immutable ledger settlement, FIFO lots, canonical withdrawal fees, fixed-point interest remainder, idempotency, and outbox; other economy mutations pending |
| 3 | Item definitions / instances / storage / equipment | Version-pinned item definitions, Item Bag stack storage, CatchBag weight accounting, pending delivery, Tool Locker, equipment reads, inspect, equip, and unequip implemented; storage-capacity purchases, Trash Recovery/discard, and later lifecycle services pending |
| 4 | Fixed NPC price/content registry | Frozen versioned resource lattice, NPC-buy/appraisal separation, Shop availability/stock classes, processed-metal formula regression, and canonical alloy recipes implemented; live Shop/NPC transactions remain pending their service slice |
| 5 | Account / Activity progression | Canonical Account XP curve/cap and fixed level Money settlement, one authoritative spendable Activity EXP pool with derived Minecraft-style level math, transaction-composable keyed Activity EXP grant/spend/loss settlement, fixed-point Rebirth utility formulas, and idempotent Rebirth reset are implemented in the progression domain; Discord command wiring and live chat/gameplay/Quest source adapters remain pending |
| 6 | Repair / Forge / Smelt / Enchant / +N / SoulBind | Pending |
| 7 | Fishing | Pending |
| 8 | Mining / depletion | Pending |
| 9 | Combat / monsters / death protection | Pending |
| 10 | Quest / stats / achievements / profile | Profile foundation only; remainder pending |
| 11 | Market / Trade / Pay | Pending |
| 12 | Clan | Pending |
| 13 | Automation | Pending |
| 14 | Events / modifier registry | Pending |
| 15 | Anti-cheat / CAPTCHA / operator case tools | Pending |
| 16 | Dedicated design-gated systems | Must remain unavailable until individually approved |

## Deliberately unavailable

The current executable does not register slash commands for unfinished gameplay systems. `/discard` remains unavailable because the authoritative source names Trash Recovery but does not define enough recovery/expiry behavior for this implementation slice to invent a live lifecycle. Storage-capacity purchases remain pending even though the baseline capacity curves are represented. The Phase 4 registry does not activate `/shop`, NPC liquidation, or stock mutation: it freezes the canonical policy data first, while exact transaction/service behavior is implemented in its later owning slice. Resource catalog entries are not prematurely activated as stack ItemDefinitions because the specification requires definition-specific stack caps but does not freeze numeric caps for these new resource definitions. The Phase 5 progression domain is authoritative for Account XP/Activity EXP/Rebirth state, and its Activity EXP transaction kernel is available to later gameplay/service owners, but `/activity`, `/level`, `/rebirth`, eligible-chat XP, and future Mine/Fish/monster/Quest progression source adapters remain unregistered until their Discord/risk/gameplay integration slices are implemented. In particular, chat XP is not activated before its duplicate/spam/automation qualification path exists, and the generic Activity EXP kernel does not invent or double-apply source-specific modifier policy. `/party` and `/casino` remain unavailable, and no implementation status should be interpreted as permission to activate systems the specification marks as requiring another design pass.

## Foundation invariants already enforced

- Player and operation identifiers are UUIDv7.
- A Discord user maps to at most one active global player record.
- ToS versions are immutable once a version number exists; a newer configured version can become current without rewriting history.
- Registration requires an explicit `accept=true` plus the exact current ToS version.
- Duplicate external Discord deliveries reuse the same operation result instead of repeating registration, starter issuance, Bank deposit/withdraw, equipment moves, stack delivery, Account XP settlement, or Rebirth.
- Operation request hashes detect accidental idempotency-key reuse with different input.
- RNG root material is persisted on every operation row before future random-domain derivation is needed.
- System-authored operations can have no Discord actor while retaining a player target, typed operation kind, policy version, request hash, and immutable ledger provenance.
- Starter tools are account-bound and represented as unbreakable/non-repairable; starter Leather armor remains breakable/repairable.
- Wallet, Bank, and liability values cannot be negative at the database layer.
- Ledger history is immutable; a deferred trigger rejects unbalanced posting sets.
- Bank deposits create holding-age lots; withdrawals consume principal FIFO.
- Bank withdrawal fee calculation uses the canonical holding-age, pre-withdrawal balance, and rolling-24-hour marginal surcharge bands with deterministic integer ceiling.
- Bank mutation state, materialized balances, ledger postings, lot state, withdrawal audit rows, and outbox events commit atomically.
- Bank interest uses the canonical 0.004%/day base rate plus the Rebirth bonus only on the first 10,000,000 Money, approaching 0.006%/day on that tranche.
- Fractional Bank-interest entitlement is retained in deterministic integer fixed-point state rather than being discarded by daily flooring; credited interest compounds back into authoritative Bank lots.
- Soft-frozen accounts continue Bank-interest accrual while hard-frozen accounts advance the accrual clock without receiving paused-period interest.
- Bank-interest settlement is serialized with Bank state, creates immutable `BANK_INTEREST` ledger provenance only when whole Money is minted, and emits its outbox event in the same PostgreSQL transaction.
- The executable refreshes due interest before user-visible Bank/balance/ledger reads and before Bank mutations, while a bounded background worker catches up inactive accounts.
- Item definitions have immutable historical versions; stateful item instances pin the exact definition version used to interpret them.
- Stack commodities are stored separately from unique ItemInstances and compute Item Bag occupancy from definition-specific stack caps.
- Item Bag starts at 36 slots and CatchBag starts at 1,000 kg; capacity math uses checked integer arithmetic.
- Capacity-safe stack delivery never silently drops valid assets: a delivery that does not fit becomes an authoritative pending asset delivery instead of mutating the bag.
- Tool Locker is modeled as a death-safe first-class item location; equipped items are separate first-class locations backed by equipment slots.
- Deferred database consistency triggers require every `EQUIPPED` ItemInstance to have exactly one owner-matching, type-compatible equipment slot and forbid non-equipped items from remaining slot-referenced.
- Equip/unequip operations lock authoritative player/item rows, are idempotent, emit immutable asset events, and commit their outbox event atomically.
- Exact storage reads remain private in the Discord adapter while the equipped loadout can be public.
- Content/price policy rows are versioned and immutable; activating a later policy moves a separate pointer instead of rewriting historical values.
- The frozen v1 registry stores `npc_buy_price` separately from canonical appraisal so appraisal-only Forge/alloy outputs cannot become NPC liquidation paths.
- Every row that is available in the normal Shop has an explicit stock-policy class; items the specification forbids from the normal Shop have no Shop sell price.
- Registry constraints reject a direct fixed NPC-buy/Shop-sell price pair where NPC buy is not strictly below Shop sell, preventing the simplest risk-free NPC arbitrage loop at the data layer.
- Processed-metal appraisal regression uses checked integer arithmetic for `round((raw + Coal/8) × 1.005)` and matches the frozen Tin/Copper/Zinc/Aluminum/Iron/Lead/Silver/Nickel/Gold/Cobalt/Titanium/Tungsten/Netherite/Platinum values.
- Bronze, Brass, Invar, and Electrum recipes are frozen as versioned content recipes; their convenience Shop prices are weekly-limited while NPC liquidation remains disabled.
- Account XP is non-spendable within a Rebirth cycle, capped at the exact Lv200 threshold of 172,370, and derives Level 1–200 from the frozen `round10(100 + 5L + 0.02L²)` curve.
- Account level-up Money is derived only from the frozen `round100(500 + 75N)` reward curve, totals 1,609,400 Money for Lv2→200, and is minted through an immutable balanced `LEVEL_REWARD` ledger transaction rather than a mutable side balance.
- Activity EXP has exactly one non-negative authoritative `activity_xp_points` pool. Activity Level and within-level progress are derived from that pool using the exact Minecraft-style piecewise curve; spending or losing AEXP can therefore lower the displayed level.
- Activity EXP grants, spends, and losses can participate directly in an owning PostgreSQL transaction. They require positive already-effective integer points, a stable per-operation mutation key, and non-empty structured provenance; duplicate replay returns the stored receipt and an input mismatch is rejected instead of double-applying the delta.
- Activity EXP transaction settlement locks in deterministic operation → player → progression order, rejects terminal operations without a matching stored sub-mutation, rejects frozen accounts, and never permits spend/loss below zero. Source-specific modifier calculation remains outside this settlement kernel so Rebirth/guild/clan/event/automation modifiers cannot be applied twice.
- A composite operation may own multiple immutable progression events, keyed independently inside the operation; legacy single-event Account XP/Rebirth writes keep the default `primary` mutation key.
- Rebirth requires Account Level 200, increments the persisted Rebirth count, resets only Account XP to zero, preserves Activity EXP exactly, grants no direct Money, and settles any due Bank interest under the old Rebirth count before changing the count.
- Rebirth AEXP-gain and Repair-time utility functions use deterministic fixed-point exponentiation for the accepted diminishing-return formulas; they do not create a generic "all buffs scale with Rebirth" rule.
- Progression events are append-only, progression mutations are operation-idempotent, and canonical state plus ledger/outbox/event provenance commits atomically under the owning operation.
- Outbox events are committed in the same PostgreSQL transaction as canonical mutation state.
- Temporary deletion cooldown identity uses a keyed HMAC fingerprint rather than storing a permanent hidden raw Discord identity tombstone.
