# Implementation Plan Deviations

This document records cases where implementation-time evidence caused Graphite to diverge from an earlier plan, plus challenged plan assumptions that were deliberately kept because no better evidence-backed design was found.

A plan is treated as a design hypothesis, not as authority. Changes recorded here must be supported by the active specification, repository state, tests, dependency/API evidence, or another authoritative source. Static suspicion alone is not enough.

Status labels:

- `CONFIRMED` — repository/spec/runtime evidence proves the planned assumption should change.
- `DISPROVED` — a suspected problem or alternative was checked and evidence does not support changing the plan.
- `UNRESOLVED` — evidence is insufficient to make a safe implementation decision; Graphite must fail closed or defer the behavior instead of guessing.

## SoulBind policy slice

Branch: `feat/soulbind-policy`

### 1. Ordinary-equipment eligibility must be explicit at the policy boundary

**Initial plan**

Use `EquipmentTier` as the primary SoulBind eligibility input and allow Netherite/Graphite tier selection to represent the eligible equipment set.

**Implementation-time finding — `CONFIRMED`**

The active specification restricts SoulBind to **ordinary Netherite or Graphite equipment**, while the current ItemDefinition/ItemInstance model has no authoritative typed `ordinary/special` discriminator. Tier alone therefore cannot prove the complete eligibility predicate.

**Final decision**

`preview_soulbind_binding` requires an already-resolved `is_ordinary_equipment` input in addition to tier. The pure kernel rejects non-ordinary equipment explicitly. A future stateful SoulBind owner must derive and revalidate that classification from authoritative versioned item data under the owning transaction; Discord input or cache state must not become authority.

No schema field was invented in this slice because the repository does not yet freeze the authoritative representation of that classification.

### 2. Percentage-fee arithmetic reuses `graphite_core::Money`

**Initial plan**

Introduce a Services-local checked integer helper for player-paid percentage fees so Slot Orb and SoulBind share identical ceiling behavior.

**Implementation-time finding — `CONFIRMED`**

`graphite_core::Money::ceil_basis_points()` already owns canonical checked non-negative percentage-ceiling arithmetic. Reimplementing the formula in `graphite-services` would create a second source of truth and unnecessary rounding-drift risk.

**Final decision**

The Services helper is only a thin whole-percent-to-basis-points adapter over `Money::ceil_basis_points()`. Slot Orb and SoulBind retain domain-specific validation/errors while the actual percentage arithmetic remains owned by Core.

Adding the existing `graphite-core` path dependency to `graphite-services` does not create a dependency cycle and follows the repository's current layering pattern.

### 3. Historical charged-appraisal watermark was removed

**Initial plan**

Persist a historical charged-appraisal/high-watermark value for SoulBound items. Appraisal decreases would not lower that watermark, preventing a later rebound below the previous high from charging again.

**Implementation-time finding — `CONFIRMED`**

The active specification freezes all of the following:

- a 60% charge for positive appraisal mutation;
- integer ceiling for player-paid percentage fees;
- `bind early + top-ups == bind late` for the appraisal-protection charge.

However, repository/spec review did **not** find an authoritative rule requiring historical high-watermark persistence or defining rebound behavior after an appraisal decrease. Persisting such state would therefore turn an implementation assumption into a gameplay/economy rule.

**Final decision**

The pure top-up kernel accepts the authoritative `previous_enhanced_appraisal` and `new_enhanced_appraisal` for the current mutation only.

For an increase:

`top_up = ceil(60% × new) - ceil(60% × previous)`

This telescopes across any monotonic increase path and preserves the explicit bind-early/bind-late invariant across integer rounding boundaries.

For equal/decreasing appraisal, top-up is zero and no refund is invented. Historical rebound semantics remain deliberately unspecified until an authoritative rule exists.

### 4. SoulBind remains an immediate policy/lifecycle domain, not a generic service job

**Challenged alternative — `DISPROVED`**

Because SoulBind is part of Phase 6 beside Repair/Forge/Smelting, implementation review considered whether it should reuse the generic `service_jobs` runtime/reservation model.

`service_jobs` currently represents long-running work with immutable reservation/runtime provenance and terminal RUNNING transitions. Binding/unbinding is instead an immediate atomic item/economy mutation with no canonical processing clock or progressive work state.

**Final decision**

Do not force SoulBind into `service_jobs`. A future SoulBind transaction should follow the repository's operation/idempotency/locking/ledger/progression conventions directly and atomically settle the item state, materials, Money, AEXP, events, and outbox effects.

### 5. AEXP settlement does not need a new SoulBind-specific primitive

**Challenged alternative — `DISPROVED`**

The progression crate already exposes transaction-composable Activity EXP grant/spend/loss settlement with stable mutation keys, authoritative non-negative enforcement, immutable provenance, and replay-safe receipts.

**Final decision**

SoulBind policy only freezes the tier-specific AEXP amount. The future owning transaction should reuse the progression settlement primitive rather than duplicate AEXP mutation logic in Services.

### 6. Regression coverage was strengthened beyond the initial example tests

**Initial plan**

Cover representative rounding examples and a small bind-early/top-up/bind-late sequence.

**Implementation-time finding — `CONFIRMED`**

The top-up invariant is specifically sensitive to integer-ceiling boundaries. A few hand-picked examples can prove examples but not give strong regression confidence across boundary transitions.

**Final decision**

A public-API integration regression exhaustively walks monotonic appraisal paths across a dense low-value range and checks that accumulated top-ups always equal bind-late liability. A second regression repeats the invariant near `i64::MAX` to prove supported boundary arithmetic remains representable and path-independent.

### 7. Dependency/toolchain audit did not justify a version bump in this slice

**Check result — `DISPROVED` for a SoulBind dependency bump**

The workspace direct dependency pins and production baseline were rechecked against current stable releases only; prerelease/beta/RC versions are excluded. No direct dependency used by this slice was proven to require a stable-version upgrade, so no unrelated dependency churn was mixed into the SoulBind commit.

The audit also checked the recent `arrayref` supply-chain concern instead of assuming `blake3` implied exposure; no branch change was justified from the current dependency evidence.

### 8. Missing committed `Cargo.lock` remains unresolved

**Implementation-time finding — `UNRESOLVED`**

This executable workspace currently does not commit a `Cargo.lock`, and `.gitignore` does not intentionally exclude it. Exact direct dependency pins reduce but do not eliminate transitive-resolution drift between builds.

A lockfile for an application/workspace would improve reproducibility, but the current implementation environment does not have Cargo available to generate and validate the lockfile. Hand-authoring one would be less trustworthy than deferring the change.

**Final decision**

Do not fabricate `Cargo.lock` in this slice. Resolve it in a dedicated reproducibility/dependency slice using Cargo, then run the full exact-head formatting, Clippy, workspace, and PostgreSQL integration gates.

## SoulBind items intentionally still unresolved

The policy slice does not invent values or persistence for the following:

- authoritative ordinary/special ItemDefinition classification representation;
- SoulBind Rune ItemDefinition, immutable definition version, and stack cap;
- persisted SoulBound state and rebind-cooldown ownership;
- authoritative ItemInstance-to-appraisal resolver and creation-roll persistence bridge;
- atomic Rune/material/Money/AEXP bind settlement;
- atomic unbind settlement and no-refund provenance;
- true-death SoulBind protection integration;
- bound Netherite → Graphite promotion integration with authoritative previous/new appraisal and atomic top-up settlement;
- repository `Cargo.lock` generation and validation.

## Equipment appraisal composition slice

Branch: `feat/equipment-appraisal-composition`

### 1. Missing creation-roll storage precision does not block exact pure policy math

**Initial plan assumption**

Treat final `EnhancedCanonicalAppraisal` composition as blocked until the repository freezes a concrete storage/fixed-point precision for immutable creation roll `q`.

**Implementation-time finding — `CONFIRMED`**

The active specification freezes `q ∈ [0,1]`, `RollFactor(q) = 1 + 0.12q²`, requires non-negative rational/fixed-point appraisal intermediates, and freezes both the `RecraftAppraisal` and final `EnhancedCanonicalAppraisal` formulas. It does **not** require the pure policy layer to choose the persistence encoding of `q`.

**Final decision**

Represent `CreationRoll` as a validated, normalized exact rational numerator/denominator in the pure Services API. The kernel chooses no gameplay precision and no database representation. Equivalent fractions normalize to the same value; zero and one are exact. A future ItemInstance resolver remains responsible for freezing how the immutable roll is stored and reconstructing the same rational value.

Arithmetic is checked and fails closed if an extreme rational representation exceeds supported `u128` intermediates; this is an implementation bound, not a gameplay precision rule.

### 2. Final enhanced appraisal uses an algebraically equivalent lower-overflow form

**Initial implementation shape**

Follow the written final formula literally by adding `EmbeddedEnchantValue` to the structural rational under a common denominator and then applying final round-half-up.

**Implementation-time finding — `CONFIRMED`**

`EmbeddedEnchantValue` is already an integer after its own frozen 70% round-half-up step. For any non-negative rational `x` and integer `k`:

`round_half_up(x + k) = round_half_up(x) + k`.

Therefore the literal formula is exactly equivalent to:

- `RecraftAppraisal = round_half_up(Base × RollFactor × UpgradeFactor)`;
- `EnhancedCanonicalAppraisal = RecraftAppraisal + EmbeddedEnchantValue`.

**Final decision**

Use the equivalent two-step form. It shares one structural calculation with Repair, guarantees the frozen `RecraftAppraisal <= EnhancedCanonicalAppraisal` invariant for non-negative embedded value, and avoids multiplying an already-integer enchant value by a potentially large structural denominator. This reduces overflow surface without changing any canonical result.

### 3. A new public generic rational framework was rejected

**Challenged alternative — `DISPROVED`**

The existing +N appraisal code already exposes the exact rational numerator/denominator needed for composition. Creating a new public generic `Rational` abstraction and refactoring all prior appraisal kernels into it would expand API surface and migration risk without a current cross-domain requirement.

**Final decision**

Keep `CreationRoll` domain-specific and keep fraction reduction/cross-cancellation helpers private to the composition module. Reuse the existing +N exact scaling output rather than duplicating `UpgradeFactor` math. Revisit a shared rational abstraction only if another implementation slice demonstrates a concrete repeated requirement.

### 4. Cross-cancellation is performed before rational multiplication

**Initial implementation possibility**

Multiply the already-scaled +N numerator directly by the Creation Roll factor numerator, then divide by the product of denominators.

**Implementation-time finding — `CONFIRMED`**

Direct multiplication can overflow `u128` earlier than mathematically necessary when factors share cancellable divisors.

**Final decision**

Reduce each fraction and cross-cancel numerator/denominator pairs before checked multiplication. Final half-up rounding compares the remainder against the half threshold instead of computing `numerator + denominator/2`, avoiding another avoidable addition-overflow edge.

### 5. Dependency/toolchain audit still does not justify dependency churn

**Check result — `DISPROVED` for an appraisal-composition dependency bump**

This slice needs no new external crate: exact rational composition, GCD reduction, checked multiplication, and half-up rounding are all implementable with the standard library and the existing Services appraisal APIs. The stable-only workspace/toolchain audit from the immediately preceding SoulBind slice remains current; no prerelease/beta/RC dependency is introduced.

**Final decision**

Do not add a bigint/rational dependency merely for convenience. Keep checked `u128` arithmetic and explicit fail-closed bounds. The separate missing-`Cargo.lock` reproducibility finding remains `UNRESOLVED` and is not hand-authored here.

## Equipment appraisal items intentionally still unresolved

This slice does not invent:

- database precision/encoding for the immutable creation roll;
- ItemDefinition/ItemInstance → canonical appraisal resolution and cache/storage policy;
- concrete enchant-definition → appraisal-class mapping;
- live recomputation hooks after +N, enchant, or tier-promotion mutations;
- the deterministic fractional-power algorithm for `UpgradeAEXP(N) = round10(20 × N^1.55)`;
- any finite +N gameplay cap absent from the specification.

## Ordinary fresh Forge policy slice

Branch: `feat/ordinary-forge-policy`

### 1. Structural-state persistence was deferred instead of inventing an authoritative classifier

**Initial next-step plan**

After finishing canonical appraisal composition, add typed ItemInstance persistence for immutable Creation Roll and mutable +N state, then expose an authoritative ItemInstance-to-appraisal resolver.

**Implementation-time finding — `CONFIRMED`**

Repository review found that ItemInstances still use generic `state JSONB` and exact versioned ItemDefinitions, but there is no authoritative typed ordinary/special equipment discriminator. The standard TierAnchor appraisal table is explicitly for ordinary equipment, while special definitions may use explicit `base_appraisal` overrides. A persistence table could store a rational roll, but it would not by itself let the resolver prove when the ordinary table is valid. The active specification also does not freeze the RNG distribution/quantization used to generate a fresh positive Creation Roll.

**Final decision**

Do not add persistence merely to create a storage location while the owning classification/generation semantics remain unresolved. Defer the ItemInstance appraisal bridge and implement the fully frozen ordinary fresh-Forge policy first. Revisit persistence when the authoritative ordinary/special classification and owning Forge/+N mutation boundaries can be modeled without guessing.

### 2. Fresh ordinary Forge resolves its own standard base appraisal

**Initial implementation possibility**

Accept an already-resolved `BaseEquipmentAppraisal` from the caller and calculate Forge material/Money/AEXP/time policy from it.

**Implementation-time finding — `CONFIRMED`**

An arbitrary caller-supplied appraisal could be a definition-specific special-item override. Passing it into an API named ordinary fresh Forge would allow a special definition to be accidentally priced as an ordinary recipe.

**Final decision**

`preview_fresh_ordinary_forge(tier, slot)` resolves `base_equipment_appraisal(tier, slot, None)` internally after validating the fresh-Forge tier/slot domain. Starter Leather, Netherite, and Graphite reject the fresh path; Netherite/Graphite remain same-ItemInstance promotions. Gold rejects armor slots because current-v1 has no Gold armor.

### 3. Creation Roll generation remains explicitly unresolved

**Implementation-time finding — `UNRESOLVED`**

The specification freezes that Fresh Forge creates a new ItemInstance at +0 with a new normal positive Creation Roll, but current source review does not freeze the distribution, discrete precision, or RNG-to-percentile mapping for that roll.

**Final decision**

The preview records `requires_new_positive_creation_roll = true` and never generates a numeric roll. A future owning Forge transaction must freeze or receive the authoritative roll-generation policy before it can create production equipment. No uniform distribution or fixed decimal precision is assumed here.

### 4. Ordinary Forge cancellation remains unspecified rather than guessed

**Check result — `CONFIRMED`**

The active specification says ordinary Forge may use recipe-specific cancellation policy, while the fresh-equipment table does not freeze a universal post-Confirm cancellation rule.

**Final decision**

Reuse `ForgePostConfirmCancellation::Unspecified`. This is not permission to cancel; it is an explicit blocker for the future owning job lifecycle until the recipe/service rule is frozen.

### 5. No dependency or schema churn is required for the pure preview

**Check result — `DISPROVED` for adding dependencies or migrations in this slice**

The fresh Forge table, exact `round1000` Money fee, material mapping, AEXP/time schedule, output +0 contract, and eligibility restrictions are implementable with existing Services types and checked integer arithmetic. No new external crate or persistence mutation is needed.

**Final decision**

Keep the slice pure and dependency-neutral. The missing committed `Cargo.lock` and ItemInstance appraisal-state persistence remain separate unresolved work rather than being mixed into this policy commit.

## +N outcome policy slice

Branch: `feat/upgrade-outcome-policy`

### 1. Hard Freeze overlap persistence was audited and deferred

**Initial next-step candidate**

After ordinary Forge policy, add authoritative Hard Freeze overlap tracking so Smelting terminal settlement no longer depends on a caller-supplied overlap duration.

**Implementation-time finding — `UNRESOLVED`**

The current `players` row owns only the present `ACTIVE` / `SOFT_FROZEN` / `HARD_FROZEN` / `DELETED` status plus account creation/deletion timestamps; neither timestamp records a freeze transition. Repository review found no authoritative status-transition history carrying the start/end timestamps needed to reconstruct overlap for an already-running service job. Adding a new history table or trigger now cannot truthfully infer when a pre-existing Hard Freeze began.

**Final decision**

Do not create fake freeze history or silently reinterpret unrelated account timestamps. Keep Smelting's existing caller-supplied authoritative overlap boundary and defer the stateful freeze-history owner until transition provenance can be introduced with explicit semantics. Advance the fully frozen +N outcome policy instead.

### 2. The +20 probability-table boundary is not a gameplay cap

**Implementation-time finding — `CONFIRMED`**

The active specification explicitly says conceptual +N progression is unlimited while the numeric success/downgrade table is frozen only through target +20. There is no authoritative continuation curve or row for +21 and above.

**Final decision**

`upgrade_base_outcome_policy` accepts target +1..+20 and returns exact reduced rational probabilities. Target +0 is invalid; +21 and above return `ProbabilityTableUndefined` with the frozen-table boundary. This is a fail-closed data boundary, not a finite gameplay maximum. The kernel does not inherit +20 probabilities or extrapolate a curve.

### 3. Protection Orb ordering is frozen but its numeric effect is not

**Implementation-time finding — `UNRESOLVED`**

The specification freezes that Protection Orb resolves before Stabilize and that even the best Orb must leave nonzero downgrade risk. Current source review did not find a canonical prevention percentage/table for the Orb itself.

**Final decision**

Expose the ordering invariant in base outcome policy but do not accept or invent a canonical Orb probability. Stabilize remains a separate 7%-per-level prevention component, and the policy does not claim a final post-Orb downgrade chance until the missing Orb magnitude is authoritative.

### 4. Sparkling and Stabilize are exact independent policy components

**Implementation-time finding — `CONFIRMED`**

The active special-enchant table freezes Sparkling as +5% **relative** +N success per level, maximum +50% relative, and Stabilize as 7% downgrade-prevention per level, maximum 70%, losing one enchant level only when it actually prevents a downgrade.

**Final decision**

Represent both with exact rational arithmetic. Sparkling multiplies the frozen base success by its relative factor and saturates at 1/1 because probabilities cannot exceed 100%; it never adds percentage points. Stabilize exposes its prevention probability independently from Protection Orb. Supplying a level above X only demonstrates the frozen effect cap and does not authorize persistence of an enchant above its separate canonical level rules.

### 5. Outcome policy remains separate from attempt cost and live mutation

**Check result — `DISPROVED` for combining all +N work into this slice**

The outcome table and modifier probabilities are fully specified, but live attempts still depend on authoritative ItemInstance +N/enchant state, deterministic RNG ownership, material/Money/AEXP settlement, Protection Orb state, and the unresolved deterministic evaluation of `round10(20 × N^1.55)` for AEXP cost.

**Final decision**

Keep this slice pure and dependency/schema-neutral. No ItemInstance mutation, downgrade, Stabilize level decay, Protection Orb consumption, RNG draw, or command is activated. The missing attempt-cost and persistence semantics remain explicit blockers for the later owning transaction.

## Update rule for future slices

Append a new section when implementation materially diverges from its plan or when a challenged alternative is important enough to prevent the same question from being reopened later. Record the initial assumption, evidence classification, final decision, and why the alternative was accepted or rejected. Do not record speculative findings as confirmed defects.

## Mending policy slice

Branch: `feat/mending-policy`

### 1. Mosaic refund policy was deferred instead of guessing what the 10% cap means

**Initial next-step candidate**

After the Grinding modifier slice, implement the Mosaic repair-refund policy because the specification names a 1% refund chance per ordinary material unit and a 10% maximum.

**Implementation-time finding — `UNRESOLVED`**

The latest master freezes that Mosaic only refunds ordinary material units after successful settlement and never refunds Money, EXP, books, Orbs, Runes, equipment, or burned fuel. It also says `1% chance per ordinary material unit to be refunded, max 10%`. Unlike Grinding, Stabilize, and Sparkling, however, it does not say `per level`, and no deeper canonical formula was found that disambiguates whether the 10% maximum caps a level-scaled per-unit probability or caps the amount/rate refunded within one settlement.

Encoding `1% × level` would therefore create a gameplay rule from an implementation assumption.

**Final decision**

Do not implement Mosaic probability composition yet. Keep its already-frozen eligibility/timing constraints as specification evidence only and defer the numeric policy until the 10% cap semantics are authoritative.

### 2. Mending is a safer finite prerequisite than Mosaic

**Implementation-time finding — `CONFIRMED`**

Mending I has a complete finite cost/applicability contract for a pure preview:

- manual Pickaxe, Fishing Rod, Sword, and per-item Armor restoration costs 5 Activity EXP per durability;
- Pickaxe/Fishing Rod Automation restoration costs 8 Activity EXP per durability;
- Automation Mending resolves before AEXP enters the machine Experience Pool;
- `NUKE_BURNOUT` blocks Pickaxe restoration until the owning expedition is terminal.

**Final decision**

Implement a pure Mending cost preview using the existing shared `EquipmentSlot` vocabulary. Automation fails closed for Sword/Armor instead of borrowing the Pickaxe/Rod 8-AEXP rate. The preview records machine-pool ordering but deliberately does not choose the future transaction that owns earned/spendable AEXP flow.

The caller remains responsible for proving authoritative ItemInstance/enchant applicability, that Mending I is actually present, that the item is not an unbreakable Starter item, that positive missing durability exists for a live restoration, and that the Pickaxe burnout flag is derived from authoritative expedition state.

### 3. No dependency, schema, RNG, or new persistence abstraction is justified

**Check result — `DISPROVED` for infrastructure churn in this slice**

The Mending cost is exact integer multiplication over an existing `i64` durability domain. The current stable-only dependency/toolchain audit found no release-version upgrade needed by this feature, and the policy requires no RNG or new persistence representation.

**Final decision**

Keep the slice dependency-neutral and non-mutating. Use checked integer arithmetic and leave ItemInstance durability mutation, Activity EXP settlement, machine Experience Pool settlement, expedition burnout persistence, idempotency/outbox ownership, and command wiring to their future stateful owners. The separate missing committed `Cargo.lock` finding remains unresolved rather than being hand-authored here.

## Bait Rack catalog cap correction

Branch: `fix/bait-rack-shop-level-cap`

### 1. Generic Shop grouping cannot override a more-specific enchant ceiling

**Initial plan assumption**

Treat every enchant in the canonical `Shop I–V + Fishing/Chest` acquisition row as having the generic normal-Shop Level V ceiling.

**Implementation-time finding — `CONFIRMED`**

The generic acquisition table includes Bait Rack in the Shop I–V family, but the more-specific fishing/bait rule freezes **Bait Rack III**, says it is Shop/common, and explicitly limits normal-Shop levels to I–III. Under the specification priority rules, the specific Bait Rack exception overrides the generic acquisition-family ceiling.

The initial catalog therefore exposed a real incorrect `Some(5)` Shop ceiling for `CanonicalEnchant::BaitRack`.

**Final decision**

Keep `NORMAL_SHOP_MAX_BOOK_LEVEL = 5` for the generic family, add the explicit `BAIT_RACK_MAX_BOOK_LEVEL = 3`, and split `BaitRack` out of the generic match arm. Unit and public-API regressions assert that ordinary Shop families still use V while Bait Rack remains Shop-eligible only through III.

### 2. A global enchant-level framework is not justified by this bugfix

**Challenged alternative — `DISPROVED` for this slice**

Repository review found no separate authoritative max-level resolver. It would be tempting to turn this correction into a global `canonical_max_level` framework covering Shop, fishing, combining, special enchants, and Master progression.

That expansion would mix different domains prematurely: this catalog currently owns acquisition/Appraisal class and normal-Shop eligibility, while Master uses a distinct I→II tier state machine and direct fishing/combine level distributions remain separate responsibilities.

**Final decision**

Fix the proven Shop-ceiling bug at its owning boundary instead of broadening the public API. If future fishing/combine implementation needs a canonical all-source level ceiling, introduce it in a dedicated evidence-backed slice and prove every enchant family exhaustively.

### 3. No dependency, schema, or toolchain change is required

**Check result — `DISPROVED` for infrastructure churn in this correction**

The fix is a constant/mapping/test correction in the existing Services catalog. It adds no crate, migration, persistence, RNG, command, or state mutation. The current stable Rust 1.98 release baseline and existing direct dependency set remain sufficient, so no prerelease/beta/RC or unrelated dependency update is mixed into the bugfix.

## Bait Rack capacity policy slice

Branch: `feat/bait-rack-capacity-policy`

### 1. Gameplay max-level authority moved out of Shop metadata

**Initial implementation plan**

Reuse the newly corrected `BAIT_RACK_MAX_BOOK_LEVEL = 3` catalog constant as the level ceiling when implementing active bait-category capacity.

**Implementation-time finding — `CONFIRMED`**

The Level III limit is fundamentally part of Bait Rack's gameplay effect contract: three native active bait-category slots, +1 per Bait Rack level, and a maximum of six. The Shop catalog only owns acquisition metadata. Making gameplay capacity depend on a Shop-specific constant would invert that ownership and allow a future acquisition change to alter gameplay semantics accidentally.

**Final decision**

The Bait Rack effect module owns `BAIT_RACK_MAX_LEVEL = 3`. The catalog's `BAIT_RACK_MAX_BOOK_LEVEL` reuses that constant for the currently identical Shop ceiling. A public regression asserts the two remain equal under the current specification without making Shop metadata the gameplay source of truth.

### 2. Absence is distinct from malformed Level 0

**Initial implementation possibility**

Accept `u8` and interpret level 0 as a Rod without Bait Rack.

**Implementation-time finding — `CONFIRMED`**

The canonical enchant exists only at Levels I–III. Silently converting a persisted/passed `0` into absence could hide malformed authoritative enchant state.

**Final decision**

The pure capacity API uses `Option<u8>`: `None` means no Bait Rack, while `Some(1..=3)` means a present valid enchant. `Some(0)` and `Some(4+)` fail closed. Capacity is therefore unambiguous: `None→3`, `I→4`, `II→5`, `III→6`.

### 3. Fishing remains unactivated

**Check result — `DISPROVED` for widening this slice into live Fishing state**

The capacity rule is fully deterministic and requires no persistence, RNG, Money, AEXP, bait inventory, cast settlement, or command state. Repository search found no existing bait-slot abstraction that needs a migration or compatibility layer.

**Final decision**

Keep this as a pure Services policy only. It records that Bait Rack is Rod-only and occupies one normal Rod enchant slot when present, but leaves authoritative ItemInstance enchant resolution, bait-category selection/consumption, Fishing runtime, and command wiring to the later Fishing owner. Phase 7 therefore remains Pending.

### 4. Stable toolchain/dependency baseline remains sufficient

**Check result — `DISPROVED` for dependency churn in this slice**

Rust 1.98.0 remains the current stable release as of this implementation pass. The slice changes no Cargo manifest and needs no external arithmetic/data crate; all capacity math is bounded integer arithmetic over the canonical I–III domain.

**Final decision**

Keep the existing stable Rust 1.98 baseline and direct dependency pins. Do not mix unrelated crate upgrades or prerelease/beta/RC versions into the Bait Rack policy commit.

## Fishing area first-unlock policy slice

Branch: `feat/fishing-area-unlock-policy`

### 1. The full §77.12 capability kernel was narrowed instead of inventing fractional-power semantics

**Initial next-step plan**

Implement §77.12 as one Fishing prerequisite slice: area progression, fish tension, effective Rod line strength, line-break probability, and over-cap catch probability.

**Implementation-time finding — `UNRESOLVED`**

Area progression is fully discrete and frozen, but the canonical line-break formula contains `(R - 1)^1.30`. Graphite's computational contract requires deterministic replayable integer/fixed-point probability accounting, and repository/spec review found no authoritative deterministic fractional-power evaluation algorithm or precision for this exponent. Implementing it with `f64::powf`, an arbitrary lookup table, or a hand-chosen fixed-point approximation would create canonical RNG semantics that the specification does not define.

**Final decision**

Do not implement the line-break numeric kernel yet. Implement only the fully frozen first-unlock/access-progression policy and keep the fractional-power evaluator as an explicit blocker for the later capability slice. This mirrors the existing fail-closed treatment of the unresolved `N^1.55` +N AEXP formula rather than weakening deterministic replay requirements.

### 2. Reuse `EquipmentTier`, but never use enum ordering for Rod progression

**Initial implementation possibility**

Create a new Fishing-specific tier enum and compare tiers by ordinal/rank to decide whether a Rod satisfies an area gate.

**Implementation-time finding — `CONFIRMED`**

`EquipmentTier` already owns the canonical Wood/Stone/Copper/Gold/Iron/Diamond/Obsidian/Netherite/Graphite vocabulary. Duplicating those names would create another mapping to maintain. At the same time, ordinal comparison is unsafe: Gold is a side-grade that explicitly satisfies Deep Sea, while Iron does not; Gold also never satisfies Abyss.

**Final decision**

Reuse `EquipmentTier` behind a Fishing-specific `FishingRodForUnlock` wrapper that separately represents the Starter Basic Rod. Reject `StarterLeather` as a non-Rod tier and encode the tiny Rod×area eligibility matrix explicitly. This keeps the shared tier vocabulary while preserving Gold's special behavior without relying on enum discriminants or a fake total progression order.

### 3. First-unlock eligibility is not live cast authorization

**Implementation-time finding — `CONFIRMED`**

The specification says area unlocks are permanent once first satisfied and Rebirth never re-locks them. The Rod column is explicitly the minimum tier **for first unlock**. Separately, the Starter Basic Rod is Pool-only. Treating the first-unlock table as a per-cast tier gate would silently turn a progression condition into a permanent equipment restriction that is not stated.

**Final decision**

Name the API and result types `FirstUnlock` and document that the future stateful Fishing owner must load persisted area access first. The pure preview decides only whether a new permanent unlock may be granted. Starter Basic Pool-only remains explicit, while any broader post-unlock cast authorization/capability rules stay with the future Fishing runtime.

### 4. Fishing area access does not create depletion state

**Check result — `DISPROVED` for reusing Mining depletion infrastructure**

The latest master explicitly says fishing areas remain renewable forever and do not have a Fishing equivalent of SeamCapacity, geological pressure, depleted probability mass, or 12-hour recovery.

**Final decision**

The policy exposes the renewable/no-depletion invariant and creates no database row, pressure state, recovery timer, or shared Manual↔Auto resource pool. Area progression is access state only.

### 5. Stable toolchain/dependency baseline remains sufficient

**Check result — `DISPROVED` for dependency churn in this slice**

Immediately before branching, verified `main` remained `2ee11cc245d820201b872fd40bbd06ff7347d193` with exact push CI #200 green. Rust 1.98.0 remains the release-stable toolchain baseline, and this discrete policy requires no new crate, migration, RNG, database, or numeric library.

**Final decision**

Keep the slice dependency/schema-neutral. Do not mix unrelated crate updates or prerelease/beta/RC versions into the area-policy commit.

## Fishing species catalog policy slice

Branch: `feat/fishing-species-catalog-policy`

### 1. Full FishInstance math was narrowed instead of inventing deterministic transcendental semantics

**Initial next-step plan**

Use the newly frozen species/area table as the direct prerequisite for a complete FishInstance kernel covering weight sampling, NPC Money valuation, and derived length.

**Implementation-time finding — `UNRESOLVED`**

The authoritative Fishing rules freeze a truncated log-normal weight distribution, the valuation term `(Weight / Wref)^0.85`, and a cube-root term in the length formula. Graphite requires deterministic replayable arithmetic, but repository/spec review found no canonical fixed-point precision, approximation algorithm, lookup table, or cross-platform evaluation contract for those transcendental/fractional operations.

Using native floating point, `powf`, an arbitrary approximation table, or a dependency-selected implementation would make canonical RNG/economy results depend on an implementation choice not frozen by the specification.

**Final decision**

Do not activate weight sampling, Money valuation, or derived length yet. Freeze only the fully discrete species/area catalog and exact ReferenceLength prerequisite. Keep the sampler and fractional/transcendental evaluators as explicit blockers for the later FishInstance/runtime slice.

### 2. Exact decimal catalog values are represented in integral physical units

**Implementation-time finding — `CONFIRMED`**

The species table and ReferenceLength table contain finite decimal kilogram/metre values that convert exactly to whole grams and millimetres for all current canonical rows. No gameplay precision is lost by storing the pure policy values in those units.

**Final decision**

Expose reference weight as integer grams and ReferenceLength as integer millimetres. Keep Base NPC value as integer Money. This avoids introducing floating-point persistence/arithmetic semantics while leaving the future FishInstance storage representation open.

### 3. Current area-pool sums are an integrity fact, not a public percentage contract

**Initial implementation possibility**

Expose a public constant declaring every canonical area pool to total `100`, since all current rows happen to sum to that value.

**Implementation-time finding — `CONFIRMED`**

The specification defines the entries as relative species weights. Later modifiers such as Rare Bait or Luck alter eligible relative weights before normalization. A public `100` total would therefore invite callers to treat the raw field as a frozen percentage representation that the specification does not promise.

**Final decision**

Do not expose a public total-weight constant. Preserve the raw relative weights and keep the current sum-of-100 assertion as test-only catalog integrity evidence.

### 4. Species rarity must not be multiplied into NPC value a second time

**Implementation-time finding — `CONFIRMED`**

The latest authoritative Fishing correction states that the species Base NPC Money value already prices rarity into the species economy value. Applying another rarity multiplier during valuation would double-count rarity and distort the economy.

**Final decision**

Keep rarity as species metadata for selection/display/policy purposes, but expose Base NPC Money value independently and document that future valuation must not apply rarity again. The unresolved weight exponent remains the only size-based valuation factor from this slice's boundary.

### 5. Stable toolchain/dependency baseline remains sufficient

**Check result — `DISPROVED` for dependency churn in this slice**

Rust 1.98.0 remains the current stable toolchain baseline and the finite catalog needs no external numeric or data dependency. Adding a transcendental/fixed-point crate merely to bypass the unresolved arithmetic contract would move an unfrozen gameplay choice into dependency behavior rather than solve the specification gap.

**Final decision**

Keep the slice dependency/schema-neutral. Do not add a numeric crate, migration, FishInstance persistence, command, or runtime state until the owning semantics are authoritative.

## Fishing capability primitives slice

Branch: `feat/fishing-capability-final-policy`

### 1. Tier alone cannot prove an ordinary Fishing Rod

**Initial implementation plan**

Resolve ordinary Rod base line strength and durability from `EquipmentTier` alone, while documenting that Starter Basic Rod is separate.

**Implementation-time finding — `CONFIRMED`**

The active specification says Starter Basic Rod is a separate system-bound, unbreakable definition and must not use the ordinary Wood durability row. Repository evidence also shows the current Starter Basic ItemDefinition carries Wood-like metadata (`"tier":"WOOD"`, `"line_strength":6`, `"ordinary_durability":600`) together with `starter_unbreakable=true`.

Therefore a tier-only `Wood` input cannot prove that the caller resolved an ordinary Wood Rod rather than the Starter Basic definition.

**Final decision**

`ordinary_fishing_rod_base_stats` requires an explicit already-resolved `is_ordinary_rod` classification in addition to `EquipmentTier` and fails closed when that classification is false.

A future stateful Fishing owner must derive and revalidate this classification from authoritative versioned ItemDefinition/ItemInstance state before calling the pure policy. The pure Services slice does not invent a new persistence discriminator merely to satisfy this boundary.

This mirrors the existing SoulBind policy rule that tier alone is insufficient when ordinary/special classification matters.

### 2. Full line-break arithmetic remains unresolved

**Check result — `UNRESOLVED`**

The exact discrete prerequisites — Rod base stats, rarity tension multipliers, and exact FishTension — are implementable with checked integer/rational arithmetic. The later line-break probability still requires `(R - 1)^1.30`, for which the repository/specification does not freeze a deterministic fixed-point precision or approximation algorithm.

**Final decision**

Keep this slice limited to exact prerequisites. Do not use native floating point, `powf`, or an arbitrary numerical dependency/lookup table to activate line-break RNG.

### 3. Stable toolchain/dependency baseline remains sufficient

**Check result — `DISPROVED` for dependency churn in this slice**

The policy uses existing `serde`/`thiserror` plus standard-library checked integer arithmetic. No new crate, migration, persistence layer, runtime, or command is required.

**Final decision**

Keep Rust 1.98.0 and the current direct dependency set. Do not mix localization/emoji infrastructure or unrelated dependency changes into this Fishing policy slice.

## Manual Fishing base AEXP policy slice

Branch: `feat/manual-fishing-aexp-base-policy`

### 1. The Fishing reward table is base AEXP, not a final progression mutation amount

**Initial implementation plan**

Expose the frozen Manual Fishing AEXP table directly as the amount to grant for Junk, Treasure, or landed fish.

**Implementation-time finding — `CONFIRMED`**

The active specification has a Rebirth Activity EXP gain bonus and a global AEXP gain-modifier cap of 1.75×. The progression mutation kernel also explicitly requires callers to provide the final integer AEXP amount after source-specific modifiers and caps; its integration regression records `modifiers_already_applied = true` in provenance. Treating the Fishing table as a final mutation amount would therefore make a future Fishing adapter capable of silently bypassing the modifier layer.

**Final decision**

Name the public Fishing functions and constants as **base AEXP**. The Services policy owns only the source table and source-local Multi Treasure cap. A future stateful Fishing settlement owner must apply the authoritative AEXP gain modifier stack after this base policy and only then pass the final positive integer grant to `apply_activity_xp_mutation`. This slice does not duplicate Rebirth/global modifier composition inside Fishing.

### 2. Heterogeneous Multi Catch AEXP cap basis remains undefined

**Initial implementation possibility**

Aggregate all landed fish AEXP and cap the result at `3 × single-cast fish AEXP` in the same helper.

**Implementation-time finding — `UNRESOLVED`**

The active specification says Multi Catch grants each successfully landed fish's rarity AEXP but caps the cast at `3× the single-cast fish AEXP`. It does not define which fish supplies that cap basis when one cast lands fish with different rarities. Using the first fish, lowest rarity, highest rarity, average rarity, or another basis would each produce different canonical progression output.

**Final decision**

Do not expose an aggregate Multi Catch AEXP function yet. Freeze only the per-fish rarity base table. The future aggregate resolver must remain blocked until the cap basis for heterogeneous landed fish is authoritative.

### 3. Multi Treasure has a finite landed-count domain and a separate base-AEXP cap

**Implementation-time finding — `CONFIRMED`**

The canonical Multi Treasure contract produces single, double, or triple Treasure; no four-or-more Treasure result is defined. Separately, Treasure pays 5 base AEXP and total Treasure AEXP is capped at 10 per cast.

**Final decision**

`manual_fishing_base_treasure_cast_aexp` accepts only authoritative landed Treasure counts `1..=3`, computes `5 × count`, and caps the source-local base result at 10. Count 0 or 4+ fails closed rather than being hidden by the cap. The helper does not own Multi Treasure RNG or infer enchant-level proc distributions.

### 4. Failed fish outcomes skip the progression mutation instead of emitting a zero grant

**Implementation-time finding — `CONFIRMED`**

Fish escape and line-break outcomes grant zero Fishing AEXP. The shared progression mutation kernel intentionally rejects non-positive mutation amounts.

**Final decision**

The base outcome policy returns `None` for `FishEscaped` and `LineBreak`. The future owning Fishing transaction must omit the Activity EXP grant sub-mutation for those outcomes rather than manufacture a zero-value progression event.

### 5. No dependency, schema, RNG, or runtime activation is justified

**Check result — `DISPROVED` for infrastructure churn in this slice**

The source table and Multi Treasure cap use bounded integer arithmetic and existing `FishingRarity`, `serde`, and `thiserror` types. Repository-wide integration review found no existing Manual Fishing AEXP source owner to migrate and no live Fishing command/runtime that needs compatibility wiring.

**Final decision**

Keep the slice dependency/schema-neutral and non-mutating. Do not add migrations, RNG draws, ItemInstance settlement, Modifier Registry implementation, progression writes, or `/fish` command wiring. Phase 7 remains unactivated until the owning Fishing lifecycle is implemented.

## Base Fishing droptable policy slice

Branch: `feat/fishing-base-droptable-policy`

### 1. Baseline catch percentages are represented as relative weights before normalization

**Initial implementation possibility**

Expose the zero-temporary-modifier 88.00% Fish / 8.50% Junk / 3.50% Treasure rows as immutable final probability values.

**Implementation-time finding — `CONFIRMED`**

The active Fishing rules state that Gold Rod, Treasure Bait, Treasure enchant, and other eligible Fishing modifiers alter branch weights relatively before shared normalization/caps. Treating the baseline rows as permanently final probabilities would invite callers to apply relative modifiers to already-final values or bypass the future shared composer.

**Final decision**

Represent the exact baseline with reduced common-scale relative weights `176 / 17 / 7`. At zero modifiers these normalize exactly to 88.00% / 8.50% / 3.50%, while preserving the correct pre-normalization ownership boundary. This slice does not compose temporary modifiers, apply shared caps, or perform RNG.

### 2. “A valid cast still always catches something” is an initial branch invariant, not a final-settlement guarantee

**Implementation-time finding — `CONFIRMED`**

The droptable has no empty initial branch, but the later Fishing capability rules can still make a Fish candidate escape or be lost to a line break.

**Final decision**

Keep the base branch enum exhaustive over Fish, Junk, and Treasure and do not add a no-result branch. Do not expose a misleading `always_settles_item` or equivalent guarantee: successful initial selection and final settlement remain separate stages.

### 3. Treasure-branch modifiers do not leak into the internal Treasure result table

**Implementation-time finding — `CONFIRMED`**

The specification explicitly says Treasure X modifies the Treasure branch relatively and does not multiply an Enchant Book's internal rarity. Multi Treasure repeats Treasure-result quantity only after a Treasure proc and result selection.

**Final decision**

Freeze the internal Treasure result table separately as reduced relative weights `19 / 13 / 5 / 4 / 5 / 4`, reproducing 38% / 26% / 10% / 8% / 10% / 8% exactly. Gold/Treasure Bait/Treasure-enchant branch modifiers are not applied again inside this table. The direct Enchant Book pool and Multi Treasure quantity RNG remain later slices.

### 4. No dependency, schema, RNG, or runtime activation is justified

**Check result — `DISPROVED` for infrastructure churn in this slice**

Both tables are finite exact integer policy data and repository review found no existing base catch/treasure droptable owner to migrate. No external numeric/data crate, schema state, RNG draw, settlement path, or command wiring is needed.

**Final decision**

Keep the slice pure, dependency/schema-neutral, and non-mutating. Phase 7 remains unactivated until the owning Fishing lifecycle and deterministic RNG composition are implemented.

## Base Fishing droptable latest-spec correction

Branch: `fix/fishing-treasure-table-latest-spec`

### 1. Newer authoritative master supersedes the initially implemented Treasure-result weights

**Implementation-time finding — `CONFIRMED`**

A post-merge source-authority audit found that the newer current Master Specification dated 2026-08-28 explicitly replaces the older within-Treasure row used by the first droptable slice. The active table is 40% Material bundle / 30% Crate-or-Chest / 8% Enchant Book / 8% Orb-or-Catalyst / 10% Rare bait-or-utility / 4% Relic-or-collectible. The catch-branch baseline 88.00% / 8.50% / 3.50% is unchanged.

Under `AGENTS.md` source precedence, the newer normative Master Specification overrides the older implementation-time decision.

**Final decision**

Keep catch-branch weights `176 / 17 / 7`. Replace within-Treasure reduced weights `19 / 13 / 5 / 4 / 5 / 4` with `20 / 15 / 4 / 4 / 5 / 2`, reproducing 40% / 30% / 8% / 8% / 10% / 4% exactly. At the unchanged 3.50% Treasure baseline, nested overall chances become 1.400% / 1.050% / 0.280% / 0.280% / 0.350% / 0.140%.

The direct-fishing Enchant Book pool remains a separate next slice and must use the same newer master (`58 / 24 / 12 / 4 / 2`, mythic `45 / 30 / 25`) rather than the superseded older table.

### 2. The correction does not activate Fishing runtime

**Check result — `DISPROVED` for wider infrastructure changes**

This correction changes only finite policy data, regressions, and source-authority documentation. It adds no RNG, migration, persistence, dependency, command, or live settlement. Phase 7 remains Pending.

## Direct-fishing Enchant Book pool policy slice

Branch: `feat/fishing-book-pool-policy`

### 1. Pool selection is frozen, but common/mid/rare member selection is not

**Implementation-time finding — `UNRESOLVED`**

The current 2026-08-28 Master Specification freezes direct Book-pool weights `58 / 24 / 12 / 4 / 2` and lists the members of Shop/common, Mid loot and Rare. It does not assign relative weights among individual enchants within those three pools. Assuming equal member probability would create a balance rule absent from the specification.

**Final decision**

Expose exact pool weights and derive pool membership from the existing canonical acquisition catalog. Do not expose a Shop/common, Mid loot, or Rare per-enchant weight/selector. The only exact member split implemented is Mythic, whose Nuke / Annihilation / Phoenix weights are explicitly `45 / 30 / 25`.

### 2. Raw pool-level profiles are not finished-book level validators

**Implementation-time finding — `UNRESOLVED`**

The master freezes raw pool-level profiles but also contains narrower enchant-specific contracts. Bait Rack is explicitly Level I–III while the Shop/common raw profile includes IV–VI; Carving is explicitly `Carving I` while the Mid loot raw profile starts at II and extends through VII. The master does not define whether a narrower member is excluded before the level roll, clamped, filtered and renormalized, rerolled, or handled another way.

**Final decision**

Expose the exact level tables as `DirectFishingBookLevelProfile` raw policy only, with fail-closed support bounds. Do not provide an enchant→finished-level resolver or invent renormalization. Mending and Phoenix remain explicit one-level profiles, while Nuke/Annihilation share their explicitly frozen profile.

### 3. Existing acquisition catalog remains the membership source of truth

**Check result — `CONFIRMED`**

`CanonicalEnchant` and `enchant_catalog_policy` already classify normal-Shop/Fishing, mid/high, rare, Fishing-only, Mythic, combine-only and Master-progression acquisition. Copying the long member lists into Fishing would create a second source of truth.

**Final decision**

`direct_fishing_book_pool_membership` maps the existing acquisition source into the five Fishing pools and explicitly excludes Shadow Walker and Master progression. Mending is handled explicitly as the current sole Fishing-only identity so a future new Fishing-only enchant cannot silently inherit Mending semantics.

### 4. Multi Treasure quantity and live RNG remain outside this slice

**Implementation-time finding — `UNRESOLVED`**

The master freezes that Multi Treasure repeats treasure-result quantity after selection, but current source review does not freeze whether repeated Enchant Books independently reroll pool/member/level or duplicate one selected book result.

**Final decision**

Do not resolve repeated Book quantity, RNG draws, ItemDefinitions, inventory settlement or `/fish` runtime here. Phase 7 remains Pending.

## Multi Treasure Level X count policy slice

Branch: `feat/multi-treasure-level-x-policy`

### 1. Only the Level X count distribution is numerically frozen

**Implementation-time finding — `UNRESOLVED` for Levels I–IX**

The current 2026-08-28 Master Specification states only the Level X result: 6% double Treasure and 1.5% triple Treasure, with expected Treasure count approximately 1.09× after a Treasure proc. It provides no table, interpolation rule, linearity rule, or other numeric mapping for Multi Treasure Levels I–IX.

**Final decision**

Expose a deliberately Level-X-only policy. The complete Level X distribution is exactly 92.5% single / 6% double / 1.5% triple, represented as integer basis points `9,250 / 600 / 150`. Do not accept an enchant-level argument and do not extrapolate Level X probabilities to lower levels. Levels I–IX remain blocked until an authoritative progression rule exists.

### 2. Multi Treasure owns the canonical maximum Treasure count

**Implementation-time finding — `CONFIRMED`**

The existing Manual Fishing AEXP policy independently encoded a maximum landed Treasure count of three only to validate its 10-AEXP-per-cast cap. Once a canonical Multi Treasure count policy exists, retaining a separate AEXP-owned `3` would create two sources of truth for the same gameplay boundary.

**Final decision**

Move the shared maximum to `MULTI_TREASURE_MAX_ITEMS = 3` in the Multi Treasure domain and make Manual Fishing AEXP reuse it. AEXP continues to own only reward math: 5 base AEXP per landed Treasure with a 10-base-AEXP cap.

### 3. Exact count probability does not activate RNG or repeated-result semantics

**Implementation-time finding — `UNRESOLVED`**

`graphite-core::DomainRng` already owns deterministic weighted sampling, but the Fishing lifecycle and RNG domain assignment are not implemented. The specification also still does not freeze whether a double/triple stateful result such as an Enchant Book independently rerolls pool/member/level for each item or duplicates the already-selected result.

**Final decision**

Keep the Level X policy as exact pure data only. Do not draw RNG, choose a domain string, mint items, duplicate/reroll Enchant Books, settle inventory, or register `/fish`. Multi Catch remains independent and is not multiplied by Multi Treasure.

### 4. No dependency, schema, persistence, or runtime change is justified

**Check result — `DISPROVED` for infrastructure churn in this slice**

The complete frozen Level X distribution fits exact bounded integer basis-point arithmetic and the existing Services layer. Repository-wide review found no current stateful Fishing owner or migration to update, and the executable still does not depend on `graphite-services`.

**Final decision**

Keep the slice dependency/schema-neutral and non-mutating. Phase 7 remains Pending; exact-head CI must verify rustfmt, Clippy, full workspace tests, and PostgreSQL integration before merge.

## Multicatch Level X count policy slice

Branch: `feat/multicatch-level-x-policy`

### 1. Only the Level X count distribution is numerically frozen

**Implementation-time finding — `UNRESOLVED` for Levels I–IX**

The current 2026-08-28 Master Specification gives only the Level X Multicatch result: 10% double, 3% triple, 1% quadruple, and 0.25% quintuple fish, with expected fish count approximately 1.20×. It does not define a table, interpolation rule, or progression formula for Levels I–IX.

**Final decision**

Expose a deliberately Level-X-only count policy. The complementary single-fish probability is exactly 85.75%, so the complete distribution is `8,575 / 1,000 / 300 / 100 / 25` basis points for single through quintuple. The weighted expected count is exactly 1.20 fish. The API accepts no enchant-level argument and does not infer lower-level scaling.

### 2. The five-fish ceiling is a global Fishing invariant, not Bait-owned state

**Implementation-time finding — `CONFIRMED`**

Before this slice, `MAX_FISH_PER_CAST = 5` lived in `fishing_bait.rs` even though the capability kernel already imported it to validate CatchLoad. Multicatch introduces another independent consumer, and the master explicitly describes five as the shared cap after School Bait and Multicatch composition. Leaving the source under Bait would make a global Fishing invariant appear owned by one modifier subsystem.

**Final decision**

Move the authoritative constant to the neutral `fishing_limits` module and preserve the existing root public API name `graphite_services::MAX_FISH_PER_CAST`. `fishing_bait` keeps only a crate-internal re-export as compatibility plumbing for existing internal imports; there is no second numeric definition. School Bait, Multicatch, and capability therefore share one source of truth.

### 3. Count policy does not define additional FishInstance identity or AEXP aggregation

**Implementation-time finding — `UNRESOLVED`**

The master says Multicatch changes fish count only and requires CatchLoad to sum FishTension across all candidate fish, but it does not specify whether extra fish independently sample species, weight, variant, and biological noise or duplicate the initial fish. Separately, the existing Manual Fishing AEXP policy already records an unresolved cap-basis question for heterogeneous Multi Catch fish rarities.

**Final decision**

Keep this slice count-only. Do not generate FishInstances, choose species/weight/variant, resolve heterogeneous Multi Catch AEXP, or invent a reroll/clone rule. The existing capability kernel remains ready to sum up to five already-resolved fish tensions once a future lifecycle owner has authoritative candidate fish.

### 4. School Bait and Multi Treasure remain independent composition stages

**Check result — `CONFIRMED`**

School Bait may add exactly one same-area fish after a Fish result, non-recursively, and is independent from Multicatch while still sharing the five-fish ceiling. Multi Treasure belongs to the Treasure branch and never multiplies Multicatch.

**Final decision**

The Level X Multicatch policy exposes count probabilities only. It does not compose School Bait, does not consume extra bait, and does not touch Multi Treasure quantity. A future Fishing lifecycle must compose independent quantity stages under the shared global cap without changing the one-normal-Rod-durability-event-per-cast rule.

### 5. No dependency, schema, persistence, RNG, or runtime activation is justified

**Check result — `DISPROVED` for infrastructure churn in this slice**

The complete Level X distribution and global count invariant are exact bounded integer data. Repository-wide review found no stateful Fishing owner or migration to update, and the executable still does not depend on `graphite-services`.

**Final decision**

Keep the slice dependency/schema-neutral and non-mutating. Do not add RNG draws/domain strings, FishInstance minting, inventory settlement, progression writes, or `/fish` command wiring. Phase 7 remains Pending.

## Shared action-speed cap primitives slice

Branch: `feat/shared-action-speed-policy`

### 1. Sharing one capped speed bucket does not freeze the within-bucket combination rule

**Implementation-time finding — `UNRESOLVED`**

The current 2026-08-28 Master Specification says action-speed modifiers share the same capped bucket instead of multiplying separately. Gold Rod, Gold Pickaxe, Lure, Efficiency, Day/Night Walker, Event and Partner sources can all contribute speed-rating inputs. The Modifier Registry contract also requires every modifier to carry an explicit `Combination Rule`, but the current source does not freeze the rule that combines those source values into one uncapped bucket total.

**Final decision**

Do not assume additive stacking. The neutral Services policy accepts an already-authoritatively-composed non-negative bucket total and only applies the frozen shared cap. Source-local policies continue to expose their own speed-rating inputs. A future Modifier Registry owner must resolve and version the combination rule before live Mine/Fish timing composition is activated.

### 2. The shared bonus cap is frozen, but rating-to-cooldown conversion is not

**Implementation-time finding — `UNRESOLVED`**

The active master freezes a 10.0-second base repeatable manual reward-action cooldown, a 7.5-second minimum Mine/Fish cooldown after speed modifiers, and a maximum shared action-speed bonus of +33.33%. Repository/spec review found no canonical formula mapping an already-resolved action-speed rating to final cooldown duration.

**Final decision**

Represent the shared cap exactly as 3,333 basis points and expose the 10,000 ms base plus 7,500 ms Mine/Fish minimum as independent timing invariants. Do not infer `base / (1 + rating)`, direct percentage subtraction, or another rating-to-duration transform. A future runtime may convert the capped rating only after that formula becomes authoritative.

### 3. The cap owner is cross-domain and does not activate gameplay runtime

**Implementation-time finding — `CONFIRMED`**

The master treats action speed as a shared modifier concern across Mining and Fishing. Existing Gold Rod and Lure policies correctly expose source-local inputs, while repository-wide search found no canonical shared cap owner.

**Final decision**

Place the cap primitive in neutral Services `action_speed` policy rather than duplicating it under Gold Rod, Lure, Fishing, or future Mining modules. Keep the slice dependency/schema-neutral and non-mutating: no cooldown persistence, clock selection, RNG, command wiring, `/fish`, or `/mine` activation is introduced. Phase 7 and Phase 8 remain Pending.

## Equipment structural state persistence slice

Branch: `feat/equipment-structural-state`

### 1. Exact structural persistence no longer requires freezing Creation Roll generation

**Earlier plan state**

Structural persistence was deferred because ordinary/special classification was not authoritative and the Fresh Forge Creation Roll distribution/quantization was still unresolved.

**Implementation-time finding — `CONFIRMED`**

The classification blocker has since been removed: immutable ItemDefinition versions now carry explicit ordinary-equipment classification and the item domain can resolve it from the exact version pinned by an ItemInstance. Separately, the pure appraisal API already defines `CreationRoll` as an exact validated `u64/u64` rational in `[0,1]`. Persisting an already-resolved rational value therefore does not require choosing how a future Forge owner samples that value.

**Final decision**

Persist the exact rational numerator/denominator and +N structural state without implementing Creation Roll generation. PostgreSQL unbounded `NUMERIC` plus explicit integer and `u64` range checks preserves the current pure-policy input domain without selecting a fixed decimal scale, quantization, probability distribution, or RNG mapping. Fresh Forge generation remains `UNRESOLVED` and no live writer is activated by this slice.

### 2. Structural state is a dedicated one-to-one row rather than generic ItemInstance JSON

**Challenged alternative — `CONFIRMED` for a dedicated row**

`item_instances.state JSONB` remains a generic read-model/state payload, while Creation Roll and +N are authoritative structural appraisal inputs. Storing the same values there would create weakly typed mutation semantics and make a future typed bridge compete with generic JSON as a second source of truth. Adding structural columns directly to `item_instances` would also couple mutable +N churn to the existing identity/location row.

**Final decision**

Use `item_instance_equipment_structural_state` as an optional one-to-one row keyed by the ItemInstance. Existing/starter ItemInstances need no backfill because no live structural writer or appraisal resolver currently requires those rows. Structural rows are constrained to equipment definition categories but are not restricted to ordinary equipment, preserving the definition-override path for special equipment.

### 3. Authoritative Creation Roll storage must use the same normalized representation as the pure type

**Full-review finding — `CONFIRMED`**

`CreationRoll::new` reduces equivalent fractions by GCD, so `1/2` is the canonical public representation of the same value as `2/4`. Allowing both encodings in authoritative persistence would create duplicate canonical representations even though they appraise identically.

**Final decision**

Require `gcd(creation_roll_numerator, creation_roll_denominator) = 1` at the database layer. PostgreSQL 18 supports `gcd` for `NUMERIC`, so this constraint does not introduce a new gameplay precision or dependency. PostgreSQL 18.6 regression coverage proves that a reduced value is accepted and `2/4` is rejected.

### 4. Fixed-scale NUMERIC was rejected because coercion could choose precision implicitly

**Implementation-time finding — `CONFIRMED`**

A fixed-scale declaration such as `NUMERIC(20,0)` can coerce an incoming fractional value to the declared scale before later constraints inspect the stored value. That would let the persistence type silently choose a rounding/quantization behavior that the specification has not frozen.

**Final decision**

Use unbounded PostgreSQL `NUMERIC` and require each persisted numerator, denominator, and +N value to equal its own `trunc(...)`, then apply explicit `u64` bounds. Fractional input fails closed rather than being rounded into an authoritative integer.

### 5. Creation Roll immutability must also block delete/reinsert replacement

**Implementation-time finding — `CONFIRMED`**

An UPDATE-only immutability trigger is bypassable by deleting the one-to-one structural row and inserting a replacement with a different Creation Roll while leaving the ItemInstance itself alive.

**Final decision**

Reject direct structural-row deletion while the parent ItemInstance still exists. Keep `ON DELETE CASCADE` on the parent relationship so a real ItemInstance deletion still removes its structural row. PostgreSQL regression coverage proves both the bypass rejection and the parent-delete cascade path.

### 6. Full `u64` storage is not a promise that every extreme value can be appraised

**Implementation-time finding — `CONFIRMED`**

The pure structural policy accepts `u64` inputs but performs checked `u128` intermediate arithmetic and deliberately fails closed when an extreme representation exceeds those supported bounds. Shrinking persistence merely to match current arithmetic headroom would turn an implementation limit into an invented gameplay cap.

**Final decision**

Preserve the complete current `u64` persistence domain for Creation Roll components and +N. Future appraisal resolution must continue to propagate checked overflow failures. Do not infer a finite +N cap or Creation Roll precision limit from the current `u128` implementation bound.

## Equipment structural state items intentionally still unresolved

This slice does not invent or activate:

- Creation Roll distribution, quantization, or RNG-to-percentile mapping;
- Forge/+N structural-state creation or mutation transactions;
- an owner-scoped transaction-composable structural-state snapshot/resolver;
- ItemInstance → `RecraftAppraisal` / `EnhancedCanonicalAppraisal` resolution;
- enchant-definition → appraisal-class mapping;
- live Repair/Forge/+N/SoulBind integration;
- a repository `Cargo.lock`.

## Ordinary equipment Recraft resolver slice

Branch: `feat/ordinary-equipment-recraft-resolver`

### 1. Full Enhanced resolution was narrowed to the authoritative ordinary structural boundary

**Initial next-step plan**

After structural-state persistence and its owner-scoped resolver, build one authoritative ItemInstance→canonical-appraisal resolver for downstream Repair, Forge, +N, Slot Orb, and SoulBind owners.

**Implementation-time finding — `CONFIRMED` for Recraft / `UNRESOLVED` for Enhanced**

The repository now has authoritative immutable ordinary/special classification, exact pinned ItemDefinition versions, and typed locked Creation Roll/+N state. Those inputs are sufficient to derive the ordinary structural `RecraftAppraisal`. However, repository/schema review still found no authoritative embedded-enchant instance/slot persistence and no concrete persisted enchant-definition→appraisal-class bridge. Treating the missing contribution as zero would turn absence of persistence into a gameplay claim.

**Final decision**

Expose only an owner-scoped transaction-composable ordinary ItemInstance→`RecraftAppraisal` resolver. Do not return or cache `EnhancedCanonicalAppraisal` until the embedded-enchant state/bridge is authoritative. The pure Enhanced composer remains available for already-resolved inputs, but this stateful bridge does not synthesize an enchant value.

### 2. Services composes appraisal policy; Items remains the authoritative storage/lock owner

**Implementation-time finding — `CONFIRMED`**

`graphite-items` already owns owner-scoped ItemInstance and structural-row locking, while `graphite-services` owns base/Creation Roll/+N appraisal policy. Making Items depend on Services would invert that layering and risk a dependency cycle as service lifecycles grow.

**Final decision**

Add a one-way `graphite-services → graphite-items` path dependency. Services calls the item-domain lock resolver, then reads only the exact immutable ItemDefinition version pinned by the locked ItemInstance. The mutable lock order remains caller operation/player locks → ItemInstance → structural row; the subsequent immutable definition read adds no competing mutable lock owner.

### 3. Current-v1 Gold armor must fail closed even though generic appraisal math is defined

**Full-review finding — `CONFIRMED`**

The shared TierAnchor×SlotFactor table can mathematically price Gold armor slots, but the active current-v1 ordinary Forge contract explicitly allows Gold only for Pickaxe, Sword, and Fishing Rod. Accepting a malformed persisted ordinary `GOLD + ARMOR` definition would therefore legitimize an unavailable current-v1 equipment shape merely because the generic math can evaluate it.

**Final decision**

The ordinary resolver explicitly rejects Gold armor before appraisal. A regression keeps Gold tools valid while proving Helmet/Chestplate/Leggings/Boots fail closed.

### 4. An ordinary `base_appraisal` parser was rejected after source-authority review

**Static-review suspicion — `DISPROVED`**

A first self-review pass suspected that calling `base_equipment_appraisal(tier, slot, None)` ignored a definition-specific override. Before retaining that change, the normative 2026-08-28 master was rechecked. It states the standard table for ordinary equipment and separately says a **special ItemDefinition** may freeze its own `base_appraisal` override. Repository history had already made the same ordinary-vs-special distinction.

No authoritative persistence representation for a special override is frozen by this ordinary resolver slice; inventing `data["base_appraisal"]` as the stateful bridge would therefore create an implementation-owned schema convention.

**Final decision**

Keep this resolver strictly ordinary and standard-table based. Reject non-ordinary definitions before appraisal. Leave special ItemDefinition override resolution to a future dedicated special-definition bridge when its persistence representation and consumers are authoritative.

### 5. Structural appraisal is shared internally without pretending embedded enchants are zero

**Implementation-time finding — `CONFIRMED`**

The first resolver shape reused `compose_canonical_equipment_appraisal(..., embedded_enchant_value = 0)` only to extract `RecraftAppraisal`. Although the returned structural value was mathematically correct, this implementation shape unnecessarily represented missing authoritative enchant state as a concrete zero inside a stateful resolver path.

**Final decision**

Factor the structural calculation into a crate-private `recraft_equipment_appraisal` helper used by both the public canonical composer and the stateful ordinary resolver. This changes no public rational model and creates no second appraisal formula; it removes the synthetic enchant input while keeping one structural arithmetic owner.

### 6. Stable-release audit did not justify dependency/toolchain churn

**Check result — `DISPROVED` for version churn**

Rust 1.98.0 and the relevant direct stable dependencies (`sqlx`, `serde`, `thiserror`, `uuid`) were rechecked before implementation. No stable update was proven necessary for this slice, and no new external crate is required.

**Final decision**

Keep the existing Rust/dependency pins. The only manifest change is the first-party `graphite-items` path dependency required by the intended layering. Do not mix unrelated dependency upgrades or prerelease versions into this resolver commit.
