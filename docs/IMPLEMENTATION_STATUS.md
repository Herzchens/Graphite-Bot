# Implementation status

This file is an evidence-based snapshot of what the repository currently implements. It is **not** gameplay/design authority. When this file conflicts with a newer explicit owner correction, the newest normative Master Spec, `docs/PLAN_DEVIATIONS.md`, or executable repository/schema/test evidence, follow the authority order in `AGENTS.md`.

Status sync baseline: runtime/code state on `main` at `1ce487b4bbb5c6ad857fc2dadef4b7fd15d35b67` (`feat(fishing): persist resolved Rod durability (#133)`). This documentation-only sync does not change runtime semantics.

## Build-order status

| Phase | Scope | Current repository status |
| --- | --- | --- |
| 1 | Identity / ToS / global player / PostgreSQL foundations | Implemented foundation. Global player identity, explicit versioned ToS consent, deletion-cooldown fingerprinting, migrations, and PostgreSQL-backed startup are present. |
| 2 | Ledger / operations / idempotency / outbox | Implemented core foundation. Operations, request hashes, RNG roots, immutable balanced Money ledger settlement, transactional outbox, Bank deposit/withdraw, Bank interest, and transaction-composable Wallet spend exist. Additional future economy domains still need their owning lifecycles. |
| 3 | Item definitions / instances / storage / equipment | Strong implemented foundation. Immutable pinned ItemDefinition versions, explicit ordinary-equipment classification, ItemInstance ownership, Item Bag/CatchBag/Tool Locker/equipment, pending delivery, structural Creation Roll/+N state, enchant slot capacities, embedded enchants, typed Favorite/Protected flags, and persisted SoulBind state are present. Equip/unequip are live and equip validates the currently implemented equipped-armor enchant conflict family. |
| 4 | Fixed NPC price/content registry | Implemented policy/content foundation through registry v3, including ordinary smelting and advanced Forge stack mappings. Live Shop/NPC liquidation and generic Forge transaction commands are not yet implemented. |
| 5 | Account / Activity progression | Implemented progression foundation. Account XP curve/rewards, authoritative spendable Activity EXP, transaction-composable AEXP grant/spend/loss and settlement prelock, Rebirth persistence/reset, and fixed-point utility formulas exist. General live progression/chat/gameplay source adapters and progression commands remain pending. |
| 6 | Repair / Forge / Smelt / Enchant / +N / SoulBind | **In progress, substantially implemented.** Authoritative appraisal/state foundations and multiple transaction-composable writers now exist. SoulBind **unbind** has a complete atomic/idempotent/auditable lifecycle and is exposed through Discord as `/unbind` / `ub`. Full SoulBind binding, live Enchant/Slot Orb/+N/Forge/Repair/Smelt lifecycles, and their commands remain incomplete for the blockers listed below. |
| 7 | Fishing | Policy foundation plus authoritative persistent prerequisites now exist: species/area/variant/drop tables, bait behavior/consumption plan, capability/over-cap routing, multicatch/multi-treasure, durability/Unbreaking-X, Gold rod, book pool, AEXP, permanent non-default area unlock ownership, and transaction-composable resolved equipped-Rod durability state. No authoritative persistent cast owner or live `/fish` command yet. |
| 8 | Mining / depletion | Pending stateful implementation. |
| 9 | Combat / monsters / death protection | Pending stateful implementation. |
| 10 | Quest / stats / achievements / profile | Basic profile surface exists; the broader Quest/stats/achievements system remains pending. |
| 11 | Market / Trade / Pay | Pending. |
| 12 | Clan | Pending. |
| 13 | Automation | Pending. |
| 14 | Events / modifier registry | Pending. |
| 15 | Anti-cheat / CAPTCHA / operator case tools | Pending. |
| 16 | Dedicated design-gated systems | Not active implementation work until their design gate is cleared. Casino and Warden are currently Deferred by explicit owner correction; Deferred systems do not block non-deferred implementation work. |

## Live Discord surface

The executable currently registers:

- `/help`
- `/tos`
- `/register`
- `/profile`
- `/balance`
- `/bank`
- `/transactions`
- `/itembag`
- `/catchbag`
- `/locker`
- `/equipment`
- `/equip`
- `/unequip`
- `/item`
- `/unbind`

Text commands use the existing `g`, `graphite`, or bot-mention prefixes. SoulBind removal also accepts the canonical text token `unbind` and alias `ub`.

A feature/policy helper existing in a crate does **not** make its gameplay command live. In particular, `/bind`, `/enchant`, live +N attempts, Slot Orb application, `/forge`, `/repair`, `/smelt`, `/fish`, `/mine`, combat, Market/Trade/Pay, Clan, and Automation are not registered as completed gameplay lifecycles.

## Phase 6: current authoritative implementation

### Equipment classification, structural state, and appraisal

The repository now has the state required to resolve ordinary equipment authoritatively inside a caller-owned PostgreSQL transaction:

- immutable ItemDefinition versions carry explicit `is_ordinary_equipment` classification;
- ItemInstances pin an exact immutable definition version;
- owner-scoped resolution locks the ItemInstance and derives classification from that pinned version;
- normalized structural state persists an exact reduced Creation Roll rational and mutable +N level;
- Normal/class and Special/universal enchant slot capacities are persisted per ItemInstance;
- embedded enchants persist canonical identity plus resulting level in typed child rows;
- ordinary Recraft and Enhanced Canonical Appraisal resolvers lock their child state deterministically and fail closed on malformed/impossible persistence.

Canonical appraisal arithmetic remains exact/integer/rational where required: standard base appraisal, Creation Roll, +N appraisal factor, embedded-enchant contribution, Recraft appraisal, and Enhanced appraisal do not use floating-point gameplay authority.

Creation Roll **generation** is still not implemented because the authoritative RNG distribution/quantization mapping is not frozen. Persisting and reading an exact roll does not authorize inventing its generation distribution.

### Enchant state

Implemented foundations include:

- canonical enchant identity/key mapping and conflict scope;
- ordinary enchant placement and slot-family policy;
- authoritative standard finished-book Apply preflight;
- authoritative embedded-enchant Apply state writer;
- exact embedded-enchant removal state writer for an already-proven-removable selection;
- authoritative Slot Orb preflight and successful slot-capacity writer;
- equipped-armor loadout validation for the implemented Guardian / Nine Life / Phoenix conflict family;
- equip-time validation of the prospective equipped armor loadout;
- dormant loadout-scoped survival-core enchant state is allowed on unequipped ordinary armor and validated at activation/equip boundaries;
- standard combine-base and selected special-enchant policy kernels.

These pieces do **not** yet form a live `/enchant` owner. Remaining lifecycle work includes authoritative book/Orb inventory settlement, Money/AEXP where applicable, deterministic operation RNG, full operation/outbox finalization, and unresolved removal/combine semantics where the active sources do not freeze numeric behavior.

Enchant removal specifically must not invent a numeric removal fee, recovery probability, removability classifier, or multi-remove composition rule where those remain unresolved. The exact state writer is only the terminal state consequence after eligibility/outcome has been authoritatively resolved.

### +N upgrade

Implemented:

- exact +N appraisal factor arithmetic;
- frozen +1..+20 base outcome rows;
- Sparkling relative success policy;
- Stabilize downgrade-prevention policy;
- authoritative ordinary upgradeability in the locked appraisal snapshot;
- transaction-composable resolved +N level writer for an already-resolved outcome.

Not implemented as a live attempt:

- complete attempt resource/Money/AEXP settlement;
- deterministic operation-owned RNG composition with all applicable modifiers;
- Protection Orb final numeric prevention composition where the active source does not freeze the missing magnitude;
- deterministic canonical evaluation for `UpgradeAEXP(N) = round10(20 × N^1.55)`;
- Discord command/lifecycle.

The frozen outcome table ending at +20 is **not** a gameplay cap. Above +20 the current probability authority fails closed because no continuation rows/rule are frozen; the repository must not extrapolate or silently inherit +20 probabilities.

### SoulBind

Persisted and authoritative SoulBind infrastructure is now present:

- one-to-one per-ItemInstance SoulBind bound/rebind-cooldown state;
- parent-lock serialization for SoulBind child writes;
- transaction-composable bind/unbind state transitions;
- typed ItemInstance Favorite/Protected flags included in the locked SoulBind snapshot;
- authoritative bind preflight foundation;
- authoritative unbind preflight;
- transaction-composable Wallet spend primitive;
- atomic unbind settlement;
- full service-owned unbind lifecycle with operation/idempotency resolution, exact committed replay, immutable Money ledger, immutable asset event, typed operation receipt, and transactional outbox;
- live Discord `/unbind` plus text `unbind` / `ub` adapter.

The live unbind path derives current Enhanced Canonical Appraisal under the owning transaction, requires the item to be currently SoulBound with Favorite and Protected cleared, charges `ceil(20% × current Enhanced Canonical Appraisal)` from **Wallet only**, refunds no prior binding resources, writes the seven-day per-item rebind cooldown, and commits all canonical effects atomically. Bank is not auto-pulled for this service fee. Replaying the same external Discord delivery returns the committed operation result instead of charging or rewriting state again; reusing the same idempotency key for different input fails closed.

Full SoulBind **binding** is still not live. Although policy, persisted state, bind preflight, transaction-composable stack consumption, and Activity EXP settlement primitives exist, the repository must still finish the owning bind lifecycle around authoritative production SoulBind Rune/material definitions and stack caps, Rebirth/package validation, Money/AEXP/material settlement, operation result/audit/outbox finalization, and the Discord adapter. `/bind` must remain unavailable until that whole path is proven.

True-death SoulBind protection and bound Netherite → Graphite promotion/top-up integration are also not yet complete lifecycle integrations.

### Repair

The pure Repair economic policy and authoritative ordinary Recraft appraisal reader exist, including frozen ordinary tier/slot material math and cancellation refund policy. No live Repair owner currently reserves/settles equipment, materials, Money/AEXP, job timing, modifiers, terminal delivery, operation/outbox, or Discord command.

Starter Leather is repairable by design, but the active canonical Repair ratio table does not freeze a Leather Money ratio. The implementation must continue to fail closed rather than borrow another tier's ratio.

### Forge

Implemented policy/content foundations cover ordinary fresh Forge and advanced Forge/promotion contracts, including same-ItemInstance promotion identity and durability projection. Transactional stack consumption now exists as a reusable primitive.

A live Forge owner is still blocked by unresolved/unfinished lifecycle authority, including fresh Creation Roll generation, complete resource/Money/AEXP settlement, operation-owned RNG for probabilistic outcomes, job/cancellation semantics where applicable, production ItemDefinition/resource bridges, terminal delivery/audit/outbox, and bound-equipment SoulBind top-up integration for positive appraisal promotion.

### Smelting

Implemented foundations include:

- ordinary Smelting heat/fuel/time/AEXP policy;
- versioned ordinary-smelting recipe mappings;
- service-job identity and reservation provenance;
- transaction-composable stack reservation;
- immutable tickless runtime snapshots;
- freeze-aware progress projection interface;
- pure terminal consequence planning;
- transaction-composable exact-version capacity-safe Item Bag delivery/pending delivery.

Smelting is not live. The remaining owner must freeze/resolve production resource ItemDefinitions and stack caps, recipe/output identity, authoritative Hard Freeze overlap tracking, atomic terminal output/raw/fuel/AEXP settlement, operation/outbox finalization, and the higher-level Confirm/command path. No preview/planner is permission to mint output or mutate assets by itself.

## Phase 7: Fishing foundation already present

Fishing is no longer accurately described as having zero implementation. The Services crate contains substantial pure policy/routing work plus authoritative persistent Fishing prerequisites, including current foundations for:

- permanent per-account access for non-default Fishing areas, with Starter Pool implicit by default;
- authoritative first-unlock resolution from persisted Account XP/Rebirth and the currently equipped Rod;
- area unlock policy;
- canonical species and per-area pools;
- fish variants;
- catch/treasure branch tables;
- bait catalog/effects and per-cast bait-consumption planning;
- rod capability, line strength, catch load, tension and over-cap resolution;
- rod durability and Unbreaking-X policy;
- authoritative transaction-composable durability mutation for the currently equipped Rod after normal wear, Unbreaking prevention, or line-break consequence has already been resolved;
- multicatch and multi-treasure;
- rod Level-X effects;
- Gold rod side-grade modifiers;
- direct fishing enchant-book pool/weights;
- manual fishing AEXP outcome policy.

The access owner persists only non-default area grants and checks persisted access before current qualification, so later Account Level/Rebirth/Rod changes do not re-lock an already-open area. First unlocks resolve the equipped Rod from the authoritative equipment slot and the exact immutable ItemDefinition version pinned by the ItemInstance; request-provided tier metadata is never authority. Starter Basic remains a separate Pool-only per-cast capability rule rather than a way to qualify a first non-Pool unlock.

The durability writer is a caller-owned transaction primitive rather than a cast owner. It re-resolves and locks the current `FISHING_ROD`, derives ordinary/special identity from the pinned immutable ItemDefinition version, uses an expected-current-durability token to reject stale state, applies exactly one ordinary durability point for a resolved normal cast unless Unbreaking already prevented it, and persists zero durability together with `is_broken` for a resolved line break or final durability point. Starter Basic preserves its canonical NULL-durability unbreakable representation and is accepted only in Starter Pool; line-break consequences fail closed in Starter Pool.

This remains **pre-command Fishing infrastructure**, not a live Fishing system. There is no authoritative persistent cast owner that snapshots the complete cast state/policy, owns domain-separated deterministic RNG, consumes bait and durability as one cast settlement, settles CatchBag/Item Bag output and AEXP, composes Mending and other terminal consequences, finalizes operation/audit/outbox, and exposes the Discord command. Permanent area access and resolved Rod-durability mutation are prerequisites of that future lifecycle, not substitutes for it.

## Important cross-cutting invariants already enforced

- PostgreSQL is authoritative canonical state; caches/read models are never mutation truth.
- Player and operation identifiers use UUIDv7.
- External Discord delivery keys are used for mutation idempotency; request hashes detect conflicting key reuse.
- Operation rows persist RNG root material before future domain-separated draws are required.
- High-value mutation owners use caller-owned PostgreSQL transactions and commit canonical state, ledger/audit records, operation result, and outbox atomically where their full lifecycle is implemented.
- Wallet/Bank/liability and Activity EXP non-negative constraints fail closed at authoritative boundaries.
- Money ledger history is immutable and balanced.
- Bank deposit/withdraw/interest behavior is operation-idempotent and uses deterministic integer/fixed-point arithmetic.
- Item definitions are immutable/versioned and ItemInstances pin exact versions.
- Owner-scoped ItemInstance resolution retains locks only for the caller transaction; classification/appraisal snapshots must not be cached and reused as later mutation authority.
- Capacity-safe stack delivery never silently drops valid assets; overflow becomes keyed pending delivery.
- Concurrent storage mutations serialize before capacity projection.
- Equip/unequip is operation-idempotent and equipment consistency is database constrained.
- Equipped armor enchant conflicts are validated at the active-loadout boundary for the currently implemented conflict family.
- Creation Roll persistence is exact and immutable after creation; generation distribution is deliberately not guessed.
- +N persistence supports the current `u64` representation domain but checked appraisal/policy arithmetic may fail closed on unsupported extreme inputs; representation bounds are not gameplay caps.
- SoulBind child writes serialize through the parent ItemInstance lock.
- SoulBind unbind is now an atomic Wallet/item/ledger/audit/outbox operation with committed replay semantics.
- Activity EXP grant/spend/loss is transaction-composable, mutation-keyed, and source-modifier calculation remains outside the generic settlement kernel to prevent double application.
- Rebirth preserves Activity EXP and resets only the account-cycle XP state defined by the progression owner.
- Permanent Fishing-area access is per account, persists only non-default areas, and is resolved before current unlock qualification so later progression/equipment changes cannot re-lock an unlocked area.
- Fishing Rod durability mutation serializes on the owning operation/player/current ItemInstance/equipment slot, fails closed on stale or malformed state, preserves Starter Basic as Pool-only unbreakable state, and writes zero durability together with Broken state.

## Deliberately unavailable / fail-closed boundaries

The following must not be inferred from nearby policy code:

- `/bind` is unavailable until the full binding lifecycle and production resource authorities are complete.
- `/enchant` is unavailable until book/Orb/economy/RNG/operation settlement is complete and unresolved numeric rules are frozen.
- live +N attempts are unavailable until attempt cost/RNG/modifier settlement is complete; no `N^1.55` approximation may be invented.
- `/forge`, `/repair`, and `/smelt` remain unavailable until their owning atomic lifecycles are complete.
- `/fish` is unavailable despite substantial Fishing policy, permanent area-access state, and transaction-composable Rod-durability state work.
- `/discard` / Trash Recovery remains unavailable while recovery/expiry lifecycle semantics are insufficiently frozen.
- generic storage-capacity purchase commands remain pending.
- resources without authoritative production ItemDefinition stack caps must not be activated merely because content/policy keys exist.
- Casino and Warden are Deferred and must stay unavailable until the owner explicitly reactivates/designs them.

## Maintenance rule

Update this file when a merged slice materially changes what is actually executable, persisted, or authoritatively transaction-composable. Do not copy every pure helper into the phase table, and do not describe a preview/preflight/writer as a live feature unless an owning lifecycle and application adapter actually expose the complete safe mutation path.
