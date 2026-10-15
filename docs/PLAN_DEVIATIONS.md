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


## Fishing owner-approved runtime policy resolutions

Branch: feat/fishing-owner-decisions

### 1. Additional fish are independent FishInstance rolls

**Owner correction — CONFIRMED**

Multicatch and School Bait never clone the base fish. Every additional fish independently rolls
species, weight, variant, and biological noise in the same Fishing area. Their quantity effects are
independent and share the global five-fish cap.

**Final decision**

Compose the two quantity effects arithmetically under one cap so evaluation order cannot change the
final count. Keep FishInstance generation itself with the future operation-owned Fishing RNG
lifecycle.

### 2. Heterogeneous Multi Catch AEXP uses the base fish as cap basis

**Owner correction — CONFIRMED**

The first/base fish owns the single-cast AEXP cap basis. Sum every landed fish's rarity base AEXP,
then cap the cast at exactly three times the base fish's AEXP. An additional rarer fish does not
raise the cap.

### 3. Repeated Multi Treasure results reroll independently

**Owner correction — CONFIRMED**

Multi Treasure preserves the already-selected outer Treasure-result category. Each repeated payload
inside that category is resolved independently. For Enchant Books, pool, member, and level are
rerolled independently for every repeated book rather than cloning one selected book payload.

### 4. Multicatch and Multi Treasure Levels I-X use linear probability curves

**Owner correction — CONFIRMED**

Multicatch Level L, for L in 1..=10, uses double 1.0% x L, triple 0.30% x L, quadruple 0.10% x L,
and quintuple 0.025% x L; single receives the remainder. Multi Treasure Level L uses double 0.60% x
L and triple 0.15% x L; single receives the remainder. Parts-per-million are the canonical integer
probability representation so 0.025% is exact. Level X reproduces the previously frozen anchor rows.

### 5. Action-speed sources add in one shared bucket and cooldown rounds half-up

**Owner correction — CONFIRMED**

Eligible action-speed ratings add before the existing +33.33% shared cap. Manual Mine/Fish cooldown
is base cooldown divided by one plus the capped rating, rounded half-up to the nearest millisecond,
then clamped to the 7,500 ms minimum. Sources do not multiply one another.

### 6. Deterministic Fishing fractional math remains a dedicated implementation slice

**Owner correction — CONFIRMED**

The future fish-weight/valuation/length and line-break kernels must use a versioned deterministic
fixed-point contract rather than native floating-point gameplay authority. This decision removes the
semantic blocker but does not make an arbitrary approximation acceptable; golden vectors and
cross-platform deterministic tests are required when that numeric kernel is implemented.


## Fishing deterministic numeric kernel v1

Branch: feat/fishing-deterministic-numeric-kernel

### 1. Log-normal center, clamp, and biological-noise semantics are owner-approved

**Owner correction — CONFIRMED**

Wref is the median of the log-normal distribution, so mu = ln(Wref) with sigma = 0.25. One
weight sample is taken and then clamped to [0.40 Wref, 2.50 Wref]; tails are not rejection-resampled.
Biological noise epsilon is an independent uniform draw on [0.95, 1.05].

**Final decision**

Numeric policy v1 maps an already-authoritative PPM draw through the midpoint of its equiprobable
bucket. The weight draw uses a deterministic fixed-point inverse-normal approximation followed by
fixed-point exp. Quality Bait multiplies the sampled-weight center by its existing 1.05 catalog
factor before the same Wref-relative clamp. The independent epsilon draw uses the same midpoint
bucket convention.

### 2. Fractional/transcendental gameplay math has a versioned fixed-point contract

**Owner correction — CONFIRMED**

Native floating point must not own fish weight, derived length, NPC valuation, or line-break
probability. The implementation algorithm may be selected and versioned by the implementation owner
as long as deterministic golden vectors protect replay semantics.

**Final decision**

Version 1 uses decimal fixed point with scale 10^12, checked i128/u128 arithmetic, deterministic
half-up/half-away intermediate division, a fixed-coefficient inverse-normal approximation, a
range-reduced integer ln/exp kernel, and fixed-point rational powers. Cross-platform regression
vectors assert exact integer outputs. No numeric dependency or Cargo change is introduced.

The FishInstance Money formula remains the latest corrected form: BaseValue multiplied by the
clamped weight-size factor and VariantMultiplier only. Species rarity is already priced into
BaseValue and is not multiplied a second time.

### 3. Line-break RNG threshold uses canonical PPM

**Owner correction — CONFIRMED**

The final line-break RNG threshold uses 1,000,000 PPM. The fractional formula is evaluated at higher
fixed-point precision, Sturdy Bait applies after the base formula, and the final probability is
rounded half-up to PPM.

**Final decision**

The pure manual line-break preview consumes the existing exact R ratio, evaluates
min(0.15, 0.002 + 0.015 * (R - 1)^1.30), applies Sturdy Bait's existing x0.90 factor, clamps to the
same 0.15 maximum, and returns the canonical PPM threshold. R <= 1 remains rejected by this over-cap
API because the existing capability owner already resolves it as zero line-break chance and
guaranteed capability landing.

### 4. Numeric policy does not invent operation RNG draw ordering

**Check result — UNRESOLVED for the stateful RNG owner**

The persisted operation RNG root is authoritative, but the repository still has no owner-approved
domain-label/index schedule for catch branch, species, variant, quantity, FishInstance weight/noise,
line-break, and later over-cap draws.

**Final decision**

Keep numeric policy v1 pure: it accepts already-authoritative PPM buckets and never reads the RNG
root. Do not expose a generic draw API from ManualFishingRngContext until the stateful cast lifecycle
can freeze a typed domain/order contract. Retry timing therefore still cannot become RNG authority.

### 5. Stable dependency baseline remains sufficient

**Check result — DISPROVED for dependency churn**

Rust 1.99.0 is now stable, while this repository is pinned to 1.98.1. This slice requires no new
language feature or external numeric crate, and upgrading the toolchain would be unrelated churn.

**Final decision**

Keep rust-toolchain.toml and Cargo dependency pins unchanged for this slice.


### 6. Sampled sub-gram weight still needs an explicit persistence/capability bridge

**Implementation-time finding — UNRESOLVED**

Numeric policy v1 deliberately preserves sampled weight at 10^12-scaled gram precision. The current
storage surface persists CatchBag fish as BIGINT `catch_weight_grams`, and the existing
`fishing_tension` capability primitive accepts whole grams. Neither the specification nor an owner
correction currently freezes whether sampled weight is rounded for persistence only, rounded before
FishTension, or represented at higher precision in canonical FishInstance state.

**Final decision**

Do not silently quantize this boundary in the numeric kernel. The future stateful Fishing owner must
freeze one canonical sampled-weight representation and its conversion into CatchBag capacity and
FishTension before it can persist FishInstances or compose capability settlement.


## Fishing typed RNG and canonical weight bridge

Branch: feat/fishing-rng-weight-bridge

### 1. Canonical sampled weight remains sub-gram gameplay authority

**Owner correction — CONFIRMED**

A sampled FishInstance keeps its 10^12-scaled gram weight as canonical gameplay state. FishTension,
CatchLoad and later capability/line-break math use that full value. The existing whole-gram CatchBag
surface is not allowed to become gameplay authority merely because it is easier to persist/display.

**Final decision**

Add an exact scaled-weight FishTension/CatchLoad path. Whole-gram CatchBag weight is derived exactly
once by positive round-half-up at the storage/display boundary. Regression coverage demonstrates a
6000.4 g Common fish is over a 6000 gram-tension Wood line even though its CatchBag projection is
6000 g; feeding the rounded projection back into gameplay would change the outcome and is therefore
forbidden.

No schema column is added in this prerequisite slice because no FishInstance persistence owner exists
yet. The future stateful owner must persist the canonical scaled weight explicitly rather than
changing the meaning of the legacy catch_weight_grams column.

### 2. Manual Fishing RNG uses semantic domain separation, not one sequential stream

**Owner correction — CONFIRMED**

Manual Fishing randomness is partitioned by fixed semantic stage and stable zero-based slot index.
Unrelated optional draws must never shift another outcome on replay.

**Final decision**

RNG policy v1 derives a fresh DomainRng for each typed stage. Global domains cover catch branch,
Treasure result, Multicatch count, School Bait proc, Multi Treasure count, line break, over-cap
catch and ordinary Unbreaking. Fish slots 0..4 independently own species, variant, weight and
biological-noise domains. Treasure payload slots 0..2 independently own Enchant Book pool/member/
level domains. Invalid slot indices fail closed before deriving a draw.

Weighted selectors consume exact authoritative integer weights. PPM stages consume one unbiased
bucket in 0..1,000,000. The raw operation RNG root remains private; adapters cannot submit arbitrary
domain strings.

### 3. Generic RNG support is extended without changing weighted-selection semantics

**Check result — CONFIRMED**

The core domain RNG previously exposed exact weighted sampling but no unbiased uniform integer range,
which the numeric FishInstance kernel requires for canonical PPM buckets.

**Final decision**

Add sample_uniform_below using the same rejection rule already embedded in weighted sampling, and
make weighted sampling delegate to it. This preserves the existing weighted draw sequence while
giving typed Fishing domains a bounded integer primitive.

### 4. No dependency or toolchain churn is justified

**Check result — DISPROVED for unrelated upgrade work**

Rust 1.99.0 is stable upstream, while 1.99.1 is still pre-release during this slice. The repository
remains intentionally pinned to Rust 1.98.1 and the current exact direct dependency set. The RNG and
weight bridge require no new crate or language feature.

**Final decision**

Keep toolchain and dependency pins unchanged. Cross-platform deterministic Fishing tests remain the
relevant compatibility evidence.


## Fishing balance-source audit and regression gate

Branch: feat/fishing-balance-regressions

### 1. Latest Master Spec already freezes aggregate Fishing economy balance

**Check result — CONFIRMED**

The latest verified 31-Aug-2026 Master Spec freezes the zero-modifier catch branch at Fish 88.00%,
Junk 8.50%, Treasure 3.50%; the within-Treasure table at 40/30/8/8/10/4; the direct Enchant Book
pool at 58/24/12/4/2; the Mythic split at 45/30/25; and the raw book-level profiles already exposed
by the Services catalog.

The same normative source includes analytic direct-book pacing at the 10-second baseline and red-team
evidence for the corrected Fish NPC Money formula: roughly 607k/h Deep Sea and 995k/h Abyss before
bait/enchant/line-break/speed effects. It explicitly rejects multiplying species rarity into Money a
second time.

**Final decision**

Treat those values as balance regression oracles rather than redesign prompts. Add a consolidated
cross-table regression that composes the existing policies and checks direct-book cadence plus a
deterministic stratified integration of the production FishInstance valuation kernel against the
Deep Sea/Abyss envelopes. Keep the six area pools stationary and exactly normalized.

### 2. Repository policy already matches the latest aggregate Fishing tables

**Check result — CONFIRMED**

Current source uses catch weights 176/17/7, Treasure-result weights 20/15/4/4/5/2, direct-book pool
weights 58/24/12/4/2, Mythic weights 45/30/25, and the latest raw level profiles. The current fish
species/area tables and variant distribution likewise match the latest Master Spec.

**Final decision**

Do not retune those tables merely because older v1.5/v1.6 documents contain earlier 38/26/10/... or
50/28/15/5/2 variants. The 31-Aug-2026 v2 correction remains authoritative.

### 3. Item-level Junk and non-book Treasure payload tables are not present in the latest spec

**Check result — UNRESOLVED**

A full-source search of the latest Master Spec finds the aggregate Junk branch and six Treasure
result categories, plus CatchBag provenance/death semantics, but no versioned item/quantity subtable
for Junk, Material Bundle, Crate/Chest, Orb/Catalyst, Rare Bait/Utility, or Relic/Collectible
payloads. The specification therefore supports their category probabilities but not arbitrary
finished asset identities or quantities.

**Final decision**

Do not invent those payload tables inside the stateful cast owner. Continue implementing safe
prerequisites and aggregate/category selection, but exact non-book asset settlement remains fail
closed until a normative content table or owner-approved amendment exists.

### 4. Common/Mid/Rare Enchant Book member weights and level-ceiling reconciliation remain unspecified

**Check result — UNRESOLVED**

The latest Master Spec lists eligible members for Shop/Common, Mid and Rare pools and gives raw
pool-level distributions, but it does not assign within-pool member weights or define how a selected
enchant's narrower canonical maximum level reconciles with the raw profile. Fixed special cases such
as Mending I, Phoenix I and Carving I exist, but they do not define a general reconciliation rule.

**Final decision**

Keep current fail-closed behavior. Do not infer equal member weights, clamp levels, reroll levels, or
renormalize filtered profiles without a normative amendment.


## Fishing canonical weight persistence bridge

Branch: feat/fishing-canonical-weight-persistence

### 1. Canonical sub-gram weight is persisted without redefining CatchBag capacity units

**Owner correction — CONFIRMED**

Canonical FishInstance weight remains the 10^12-scaled gram value used by FishTension and later
gameplay math. The existing `catch_weight_grams` BIGINT remains the whole-gram CatchBag
capacity/display projection and must not be reinterpreted as scaled weight.

**Final decision**

Add nullable `item_instances.catch_weight_scaled_grams BIGINT`. A database CHECK requires any
present scaled value to be positive, requires the legacy whole-gram projection to be present, and
requires that projection to equal positive round-half-up(scaled / 10^12). The arithmetic uses
quotient/remainder form so the consistency check does not add 5e11 to the BIGINT and create an
avoidable overflow boundary.

Existing historical catch rows remain valid with a NULL scaled value. The migration performs no
backfill because reconstructing lost sub-gram precision from a whole-gram projection would invent
biological state.

### 2. Read models expose canonical weight while capacity accounting stays whole-gram

**Implementation-time finding — CONFIRMED**

CatchBag capacity is specified and already persisted in whole grams, while the owner-approved
FishTension contract needs canonical sub-gram weight. Reusing one field for both meanings would let
display/storage rounding change capability outcomes.

**Final decision**

Expose the optional canonical scaled weight on CatchBag and Item read models, but continue summing
`catch_weight_grams` for CatchBag capacity. PostgreSQL regression coverage keeps legacy NULL rows
readable, verifies a 6000.4 g canonical fish consumes 6000 whole CatchBag grams, and rejects a
6000.5 g canonical value paired with a 6000 g projection.

### 3. This migration does not create a FishInstance minting authority

**Check result — CONFIRMED**

The repository still lacks the stateful manual Fishing cast owner and exact non-book Junk/Treasure
payload tables. Adding a storage field does not authorize an adapter or generic Item service to mint
FishInstances independently.

**Final decision**

Keep creation/settlement outside this slice. The future Fishing owner must write the canonical scaled
weight and its whole-gram projection atomically with the complete FishInstance state, CatchBag
capacity validation, progression, Rod/bait consequences, operation result, audit and outbox.


## Fishing payload and direct-book gap resolution

Branch: feat/fishing-payload-and-book-policy

### 1. Shop/Common, Mid and Rare direct-book members use equal weight within their selected pool

**Owner delegation — CONFIRMED**

The owner delegated the previously unresolved within-pool member weighting decision. The outer direct-book
pool split already freezes acquisition rarity at 58/24/12/4/2, while Mythic separately freezes its
45/30/25 identity split.

**Final decision**

Shop/Common, Mid and Rare use equal relative weight for every eligible canonical member after the pool is
selected. Mending is a singleton. Mythic keeps 45/30/25. This avoids creating a hidden second rarity layer
inside pools whose member lists, rather than member-specific weights, are the normative distinction.

### 2. Book-level ceilings filter the generic profile; singleton rules override it

**Owner delegation — CONFIRMED**

A selected enchant's more-specific canonical resulting-level ceiling remains authoritative over the
generic raw pool profile.

**Final decision**

For ordinary directly fishable enchants, remove raw-profile levels above the enchant's canonical ceiling
and sample the surviving original integer weights. The weighted selector therefore renormalizes legal
support implicitly without moving rejected mass onto the maximum level. Bait Rack III consequently keeps
Shop/Common relative weights 34/28/20 for I/II/III.

Specific singleton contracts override a generic pool profile. Carving always resolves as Carving I even
though the Mid raw profile begins at II; Mending and Phoenix likewise remain I. An unsupported or
non-direct-fishing enchant still fails closed.

### 3. Junk and non-book Treasure categories now have exact v1 catch-lot payload tables

**Owner delegation — CONFIRMED**

The owner delegated the missing exact payload identity/quantity tables. These tables are deliberately
conservative because Fishing already has a large Fish NPC Money issuance path and the missing categories
must not become an undocumented second liquid-Money faucet.

**Final decision**

Junk:
- Driftwood 40%, Rusted Can 30%, Torn Net 20%, Broken Hook 10%; one item each.

Material Bundle:
- Common 65%, Refined 25%, Deep 10%; one sealed bundle each.

Crate/Chest:
- Weathered Crate 70%, Ironbound Chest 25%, Sunken Cache 5%; one sealed container each.

Orb/Catalyst:
- Enchant Catalyst Fragment 60%, Normal Slot Orb Fragment 25%, Special Slot Orb Fragment 15%; one
  fragment each.

Rare Bait/Utility:
- School Bait 10%, Quality Bait 15%, Rare Bait 30%, Treasure Bait 25%, Sturdy Bait 20%; one unit each.

Relic/Collectible:
- Ancient Coin 50%, Sea Glass 30%, Fossil Shell 15%, Abyssal Idol 5%; one collectible each.

Each within-category table sums to 100 and is sampled independently for each Multi Treasure repetition
inside the already-selected outer Treasure result.

### 4. Material bundles and containers are sealed assets, not hidden immediate material issuance

**Design decision — REVERSIBLE**

The current content registry has no authoritative opening table for a Material Bundle, Crate or Chest.
Expanding those labels directly into mining/forge resources would silently create a new cross-system source
and could undermine Mining scarcity.

**Final decision**

The Fishing cast settles exact sealed catch-lot ItemDefinitions for those categories. Opening contents,
fragment-combination thresholds and collectible utility remain separate future item-service contracts.
All newly introduced payload ItemDefinitions keep neutral COMMON item rarity; the within-category tables
own their selection rarity, and this slice does not invent a second generic item-rarity effect. They do
not block the fish command's result identity or capacity settlement, and undeclared downstream contents
must not be inferred by the cast owner.

### 5. Stable toolchain review does not justify unrelated churn

**Check result — DISPROVED for version churn**

Rust 1.99.0 is stable upstream, while this repository remains pinned to 1.98.1. This policy/content slice
requires no new dependency or language feature.

**Final decision**

Keep the existing toolchain and dependency pins unchanged.


## Fishing active-bait persistence and cast consumption

Branch: feat/fishing-active-bait-state

### 1. Active bait selection is canonical persisted player state

**Implementation finding — CONFIRMED**

The pure bait-consumption planner could not be authoritative by itself because no persisted active-bait
selection existed. The Master Spec requires a bounded active selection, missing-bait auto-detach and
one-unit consumption per active category per cast.

**Final decision**

Persist active bait by canonical bait identity plus exact ItemDefinition version. Selection replacement
locks the owning operation/player/current Rod, derives the authoritative Bait Rack capacity from the
equipped Rod, and writes the complete replacement atomically. Request callers cannot provide arbitrary
definition keys.

Selection does not require current Item Bag possession. A missing selected bait is handled by the cast
rule itself: it auto-detaches instead of making configuration state an inventory reservation.

### 2. Per-cast bait consumption is a transaction-composable child mutation

**Implementation finding — CONFIRMED**

Bait is Item Bag stack state, so a cast owner must not decrement it through ad-hoc SQL or a second
operation.

**Final decision**

The cast bait settler locks the active selection under the owning player lock, reads exact-version
Item Bag availability for planning, then delegates every one-unit decrement to the shared
stack-consumption mutation primitive so definition/stack locks are acquired in that primitive's
canonical order. Missing selections are removed in the same transaction and one replayable
bait-settlement audit receipt records the result. Multi Catch and Multi Treasure therefore cannot create extra bait consumption.

Consuming the final available unit leaves the bait selected for that completed cast; the following cast
observes zero stock and auto-detaches it. This preserves the existing planner contract and avoids
retroactively removing an effect from the cast that consumed the last valid unit.

### 3. Capacity shrink fails closed

**Implementation decision — CONFIRMED**

A player can change to a Rod with fewer active bait-category slots after configuring bait. The
specification does not define an ordering for silently choosing which active effects to discard.

**Final decision**

If persisted active selection exceeds the currently equipped Rod's capacity, cast bait settlement fails
closed. No bait is consumed or auto-pruned. A later configuration surface can ask the player to choose
which categories remain; the backend does not invent that choice.

### 4. Bait ItemDefinition v1 is pinned explicitly

**Compatibility decision — CONFIRMED**

The current Fishing bait policy and the payload ItemDefinitions introduced by migration 0023 are v1.
Silently treating a future bait ItemDefinition version as equivalent could change gameplay effects
without a policy-version decision.

**Final decision**

Persist and consume bait ItemDefinition version 1 explicitly in the current resolver. A future bait
content version must add an explicit compatible resolver/migration rather than inheriting v1 semantics.


## Fishing unique FishInstance delivery and CatchBag overflow

Branch: feat/fishing-fish-instance-delivery

### 1. Every canonical fish species now has an exact unique ItemDefinition identity

**Implementation finding — CONFIRMED**

The FishInstance numeric kernel and canonical weight persistence existed, but the repository still had
no versioned ItemDefinition identities for the 22 canonical fish species. A stateful cast owner cannot
safely mint a unique asset from an unversioned string convention.

**Final decision**

Add one non-stackable v1 `FISH` ItemDefinition for every canonical species and pin the species-to-key
mapping in Services. Definition rarity matches the canonical species rarity. Per-instance weight,
length, variant, biological draw state and NPC value remain instance state rather than being copied
into immutable definition balance data.

### 2. FishInstance biological state is persisted as one unique non-transferable asset

**Normative alignment — CONFIRMED**

The latest Master Spec requires FishInstances to retain species, weight, length, catch time/location,
rarity and variant, and explicitly states that fish are non-transferable while remaining directly
NPC-sellable.

**Final decision**

The typed delivery primitive creates exactly one ItemInstance per resolved fish slot, marks it
account-bound/non-tradeable but sellable, and persists the complete deterministic numeric inputs and
outputs in `state`. The canonical 10^12-scaled gram weight and the database-checked whole-gram
CatchBag projection are written together. The owning operation's persisted `created_at` is the catch
time, so retry timing cannot change immutable biological metadata.

The primitive does not choose species/variant, draw RNG, consume bait, mutate Rod durability, award
progression or finalize the operation. Those remain responsibilities of the future single cast owner.

### 3. Full CatchBag preserves the unique asset through item-linked pending delivery

**Normative alignment — CONFIRMED**

Graphite overflow rules prohibit silent asset loss: valid overflow uses TEMP_OVERFLOW/pending delivery.
The existing pending-delivery schema previously represented stack rewards only and therefore could not
identify a unique FishInstance.

**Final decision**

Extend pending delivery with an optional unique `item_instance_id`. When a resolved fish does not fit
the current CatchBag weight capacity, create the FishInstance in `TEMP_OVERFLOW` and link a quantity-1
pending delivery targeting CatchBag. Existing stack pending deliveries remain unchanged with a NULL
instance link.

CatchBag read state now reports pending-delivery count so overflow is visible rather than hidden.

### 4. Pending CatchBag output bounds further catch generation

**Safety decision — CONFIRMED**

TEMP_OVERFLOW is explicitly bounded internal state. Allowing unrelated later casts to keep adding
unique fish while an earlier CatchBag delivery remains unresolved would turn it into an unbounded
secondary inventory.

**Final decision**

A pending CatchBag delivery from an earlier operation blocks new FishInstance delivery. Additional
fish slots belonging to the same already-running cast may finish settling, preserving atomic
multi-catch behavior while bounding unresolved overflow to one cast's output domain.

### 5. CatchBag capacity uses the whole-gram projection only at its storage boundary

**Regression decision — CONFIRMED**

The owner-approved numeric contract keeps canonical scaled weight authoritative for FishTension while
CatchBag capacity/display remains whole grams.

**Final decision**

The delivery path locks current CatchBag instances, sums their existing whole-gram projections, and
checks the new fish's one-time round-half-up projection against the persisted CatchBag capacity curve.
It never feeds that rounded number back into FishTension, valuation or length math.


## Fishing pending unique-catch claim

Branch: feat/fishing-pending-catch-claim

### 1. Pending unique fish are reclaimed FIFO, never recreated

**Implementation finding — CONFIRMED**

Fish overflow now preserves the original unique ItemInstance in TEMP_OVERFLOW. Resolving that overflow
must therefore move the same identity rather than minting a replacement from definition/quantity fields.

**Final decision**

Add a transaction-composable pending-catch claim primitive. It selects item-linked PENDING CatchBag
deliveries in created-order, validates each linked ItemInstance is the same player's unique FISH asset
in TEMP_OVERFLOW, and moves the original ItemInstance into CatchBag while marking the pending row
CLAIMED.

### 2. Claim stops at the first fish that does not fit

**Safety decision — CONFIRMED**

Skipping a large older pending fish to admit smaller later fish would silently create a hidden
reordering/fairness policy and make replay/capacity behavior depend on later outputs.

**Final decision**

Claim is strict FIFO. Current CatchBag usage is locked once, projected with checked integer arithmetic,
and the pass stops at the first non-fitting pending fish. Later rows remain untouched. This makes
capacity pressure deterministic and preserves pending order.

### 3. Claim is a child mutation of the later owning operation

**Lifecycle decision — CONFIRMED**

The original catch operation is already the provenance owner of the FishInstance. A later capacity
resolution must not reopen or rewrite that historical operation.

**Final decision**

Claim uses the current owning operation for mutation-key replay/audit while leaving the original
FishInstance creation operation unchanged. Rollback restores ItemInstance location, pending state and
claim audit together. A committed retry returns the stored claim receipt without moving the fish twice.


## Fishing successful-cast progression settlement

Branch: feat/fishing-progression-settlement

### 1. Successful committed manual Fishing grants exactly 2 Account XP

**Normative alignment — CONFIRMED**

The latest Master Spec source matrix assigns exactly 2 Account XP to a successful committed manual
Fishing action. This is independent of fish rarity, Multi Catch count and Treasure multiplicity.

**Final decision**

Add a transaction-composable successful-outcome settlement primitive that always requests exactly
2 Account XP through the canonical Account XP settlement owner. The primitive does not commit the
operation itself, so rollback removes the Account XP child mutation and any synchronous Account Level
Money reward together with the rest of the cast.

### 2. Fishing Activity EXP reuses the already-frozen outcome table and caps

**Normative alignment — CONFIRMED**

The exact base Manual Fishing AEXP table is Junk 2, Treasure 5, Common 3, Uncommon 4, Rare 5,
Epic 7, Legendary 10 and Mythic 14. Multi Catch uses each landed fish rarity but is capped at
3 times the base fish's AEXP; Multi Treasure caps total Treasure AEXP at 10 per cast.

**Final decision**

The progression settlement owner accepts only successful landed outcomes and delegates AEXP
calculation to the existing Fishing AEXP policy. It then grants the exact resulting integer through
the shared Activity EXP mutation owner using one stable mutation key.

Escaped-fish and line-break outcomes are deliberately absent from this successful-outcome type.
They receive zero Fishing AEXP and cannot accidentally receive the successful-manual-Fishing
Account XP child through this API.

### 3. Progression replay remains owned by shared progression primitives

**Lifecycle decision — CONFIRMED**

Fishing must not duplicate Account XP cap/level reward arithmetic or Activity EXP point mutation.

**Final decision**

The wrapper composes the existing account and activity child mutations in one caller-owned
transaction. Exact replay returns the original child receipts, a rollback removes both, and a
committed operation may replay only the already-recorded rewards. No Fishing-specific progression
tables or alternate arithmetic are introduced.


## Fishing manual cast preparation and non-Fish payload RNG

Branch: feat/fishing-manual-cast-owner

### 1. The cast owner cannot expose an early commit boundary

**Implementation finding — CONFIRMED**

Area-first-unlock and active-bait consumption are already caller-transaction mutations, while the
later branch selection, terminal Rod/Mending consequences, progression and operation/outbox commit
are not yet composed. A callable partial "cast" must not make those child effects permanently visible.

**Final decision**

Introduce an owned, non-committable pending manual cast value. It locks the persisted operation RNG
root, progression/area/Rod preflight and active bait inside one PostgreSQL transaction, deriving
known pre-cap Gold-Rod/Treasure-Bait modifiers from locked state and the actual consumption receipt.
It exposes explicit rollback and deterministic previews but no live command or commit/finalization.
Dropping the pending value rolls all interim writes back.

### 2. Junk and repeated non-book Treasure require disjoint deterministic RNG domains

**Implementation finding — CONFIRMED**

The frozen Junk/within-category Treasure tables have independent integer member weights, and
Multi Treasure rerolls each payload under the one already-selected outer Treasure result. The
previous typed RNG schedule covered the outer Treasure result and Enchant Book slots, but not
Junk or non-book member payloads; borrowing an existing domain would couple unrelated outcomes.

**Final decision**

Extend the existing stable manual Fishing v1 domain family additively with
`fishing.manual.v1.junk.payload` and
`fishing.manual.v1.treasure.{slot}.nonbook_payload` for slots 0–2.
No existing domain label or golden draw vector is changed. The selector uses the canonical
v1 integer weights, optionally resolves Multi Treasure quantity from its exact level I–X PPM
curve, and independently samples each payload at its fixed slot. A direct Enchant Book result
fails closed in this resolver and remains owned by its specialized item/enchant settlement.

### 3. Non-Fish delivery is a bounded, caller-transaction child mutation

**Implementation finding — CONFIRMED**

Unique FishInstance delivery, Junk stack issuance and sealed Treasure payload issuance have
different asset representations. A generic item mint or new top-level operation would break
version-pinning, storage-capacity conservation or the existing operation replay keys.

**Final decision**

Validate the entire resolved Junk/Treasure payload batch against the canonical result pool and
three-slot bound before any SQL write. Delegate each fixed v1 stack sub-delivery to the shared
Item Bag mutation owner with independent per-slot keys, exact definition identity and pending
overflow conservation. The full cast owner must still choose the branch from final capped
weights and compose durability, progression, result/audit/outbox before any transaction commit.


## Fishing unmodified FishInstance candidate preview

Branch: feat/fishing-fish-cast-candidates

### 1. The base Fish tables can resolve exact deterministic candidates without guessing modifier caps

**Implementation finding — CONFIRMED**

The canonical six-area species tables and five variant probabilities have exact integer weights.
The existing versioned FishInstance numeric kernel already owns weight/length/value rounding,
and the persisted manual Fishing RNG context pins independent species/variant/weight/biological
domains per fish slot. This supports an unmodified preview; it does not freeze how Luck,
Luck of the Sea, Rare/Quality Bait, Gold side-grade or other eligible modifiers combine with
future shared caps.

**Final decision**

For an expressly unmodified cast, select species from the area's canonical raw integer
pool and variants from the exact common-scale base weights 188/6/3/2/1; draw weight and
biological quantiles from the pre-existing per-slot versioned RNG domains and delegate
the deterministic transforms to `resolve_fish_instance_numeric_v1`.
An unmodified preview does not make the catch branch, capability success or issuance
authoritative. The pending cast owner refuses to use the baseline selector whenever
any active bait, Gold side-grade or embedded enchant might require the shared composer.

### 2. Starter Pool baseline Fish staging remains uncommittable

**Lifecycle finding — CONFIRMED**

The current Starter Pool Fish capability route guarantees physical landing, but choosing
the Fish branch, settling durability/progression and finalizing operation/audit/outbox still
belong to the unfinished complete cast owner.

**Final decision**

Allow the non-committable pending cast to provisionally stage only fish slot 0 for an
unmodified Starter Pool candidate, using the existing unique FishInstance delivery child
and its operation-specific mutation key. A same-operation repeat returns the original
ItemInstance receipt. Dropping or rolling back the pending cast removes the ItemInstance,
delivery event, bait and any area unlock together. No adapter may treat this preview as
a finished cast or commit its transaction before the full lifecycle gate passes.

  
## Fishing unmodified capability composition

Branch: feat/fishing-unmodified-capability

### 1. Canonical FishInstance weight, not CatchBag projection, drives physical capability

**Normative alignment — CONFIRMED**

The frozen Fishing model defines FishTension from canonical scaled fish weight and rarity,
summing across fish candidates before dividing by exact effective Rod line strength.
The existing numeric FishInstance owner and capability kernel already implement this
without discarding sub-gram precision.

**Final decision**

An unmodified single-fish candidate first validates membership in the selected area's
canonical species pool. Starter Pool Fish bypasses capability and always physically lands.
Outside Starter Pool, the caller must hold an ordinary Rod: the preview computes scaled
FishTension and one-fish CatchLoad exactly, obtains ordinary unmodified line strength, and
classifies R <= 1 as landed or R > 1 as over-cap, using the existing rational owner.

### 2. Over-cap probability previews do not authorize a terminal result or asset issuance

**Implementation finding — CONFIRMED**

The line-break numeric policy provides a versioned final PPM chance, while the over-cap
catch policy returns an exact rational chance with a 15–95% clamp. The canonical sequence
requires resolving line break first, and only after survival may the over-cap catch
resolution determine landing or escape.

**Final decision**

For R > 1 the pure preview exposes both existing probability contracts but does not
draw either RNG stage or invent a conversion from rational catch probability to a
PPM threshold. A future full cast resolver must preserve the order and version the
probability-to-draw interpretation. The non-committable pending cast can call this
pure preview from its pinned unmodified FishInstance candidate. Provisional Starter
Pool staging is now explicitly guarded by the guaranteed physical capability route.
No transaction commit, terminal operation, progression, Mending or Discord command
is enabled by this slice.


## Fishing unmodified line-break-first stage

Branch: feat/fishing-unmodified-linebreak-stage

### 1. Terminal ordering preserves canonical LineBreak domain and PPM threshold

**Normative alignment — CONFIRMED**

The versioned manual Fishing numeric kernel already produces exact integer final
line-break chance in parts per million for R > 1. The pinned manual RNG owner
exposes an independently derived `LineBreak` 0..999,999 draw, and the canonical
capability route explicitly orders line-break before over-cap catch/escape.

**Final decision**

The unmodified cast may preview whether the line broke by comparing one stable
operation-pinned LineBreak PPM bucket with the revalidated exact-ratio final
probability (`draw < chance`). Starter Pool and R <= 1 bypass that draw.
After line survival, represent the state explicitly as pending over-cap catch
selection; never report landed Fish or issue a completed cast solely on survival.
This provisional result authorizes no operation finalization, durability, payout,
outbox or Discord command.

### 2. Over-cap exact rational chance is not silently rounded to PPM

**Implementation finding — UNRESOLVED**

The over-cap catch policy stores its exact rational probability, while
`ManualFishingRngContext::sample_ppm_v1` returns a 1,000,000-domain integer.
The current source does not freeze a rational-to-PPM threshold conversion,
and no legacy helper should silently supply different rounding semantics.

**Final decision**

Keep the `OverCapCatch` RNG stage unexecuted in this slice. The full owner must
resolve the exact probability mapping and conditional draw ordering, then compose
the immutable branch/outcome with the remaining asset, Rod, progression,
operation-result and audit/outbox writes in the same PostgreSQL transaction.


## Fishing provisional non-Fish output and progression composition

Branch: feat/fishing-non-fish-cast-staging

### 1. Child delivery and reward mutations share the locked cast operation

**Implementation finding — CONFIRMED**

The canonical Junk and non-book Treasure payload selectors now have stable operation-seeded
identity and repeat-slot domains. The existing bounded Item Bag delivery and successful
Fishing Account XP/Activity EXP services are transaction-composable and replay-safe. Issuing
them in separate transactions would violate the future all-or-nothing cast boundary.

**Final decision**

On a provisional Junk or non-book Treasure branch, resolve the canonical payload identity,
stage the exact quantity of item outputs, then stage exactly 2 Account XP and the frozen
Junk/Treasure Activity EXP result under the original operation and PostgreSQL transaction.
Use the existing one-time mutation keys and shared progression owners. On a duplicate call,
require exact stored child receipt replay; don't allocate or grant twice.

### 2. Provisional branch selection never authorizes commit or external command handling

**Safety finding — CONFIRMED**

The branch parameter in this pending API is not produced from the final capped catch
distribution. Ordinary Rod durability, Mending and operation-result/audit/outbox remain
unresolved. Treating this provisional selection as a completed cast could issue rewards
for an outcome that the canonical RNG would never have selected.

**Final decision**

The pending cast retains exclusive ownership of its transaction and deliberately offers no
commit or extraction path. Direct Enchant Book Treasure fails closed before stack delivery;
Fish likewise cannot enter the non-Fish staging API. A rollback must remove any first-area
unlock, bait consumption, Item Bag or pending output, Account XP, Activity EXP and asset/
progression events together. Only a future terminal-complete owner may finalize the operation
and expose `/fish`.


## Fishing baseline catch-branch RNG selection

Branch: feat/fishing-baseline-branch-owner

### 1. Zero-modifier catch distribution has one exact versioned RNG authority

**Normative alignment — CONFIRMED**

The canonical zero-temporary-modifier manual Fishing distribution is 88% Fish,
8.5% Junk and 3.5% Treasure. The existing base drop-table policy preserves the
reduced integer weights 176/17/7, and the manual Fishing RNG owner exposes an
operation-seeded `CatchBranch` weighted domain.

**Final decision**

For a cast whose locked state contains no consumed bait, no embedded Fishing Rod
enchant and no ordinary Gold Rod side-grade, derive the branch from those exact
canonical weights using the pinned `CatchBranch` domain. A second preview of the
same operation must select the same branch and never draw from another stage.
Share the unmodified-state guard with the existing base FishInstance preview,
so a modified cast cannot silently use baseline branch probabilities.

### 2. Baseline branch preview is not general modifier normalization

**Boundary finding — CONFIRMED**

Treasure Bait, Gold Rod and Rod enchants change relative branch odds. Their
pre-cap factors are not proof of a complete shared Fishing caps/normalization
contract, and any currently absent global/Event modifier authority also has
to be reconciled before runtime activation.

**Final decision**

Only zero-modifier baseline branch selection is exposed for internal pre-command
composition. It does not grant the caller a right to commit provisional item/XP
stages or interpret a catch as terminal. Modified cases remain fail-closed until
the full caps, enrichment and terminal owner is frozen; `/fish` remains disabled.


## Fishing Starter Basic selected-branch staging

Branch: feat/fishing-starter-base-outcome

### 1. Selected baseline branch determines staged output and successful XP

**Implementation finding — CONFIRMED**

A no-modifier Starter Basic cast at Starter Pool has a canonical Fish/Junk/Treasure
distribution, guaranteed physical landing for Fish, and no ordinary Rod durability
event because the separate Starter Basic definition is unbreakable. The canonical
FishInstance, non-book payload delivery and successful progression child services
are already exact and transaction-composable.

**Final decision**

In that narrow baseline, use the operation-pinned `CatchBranch` draw (not an
adapter-supplied branch) and stage exactly the selected outcome: Fish slot 0 is
minted via the unique FishInstance child and its rarity earns the canonical Fish
AEXP; Junk or non-book Treasure uses the existing pinned payload and AEXP path.
Every successful staged candidate uses exactly 2 Account XP. A retry in the same
pending transaction returns the original item and XP receipts. The cast remains
uncommittable, with no live command or completed-result authority.

### 2. Fail closed on unsatisfied Rod and direct-book paths

**Safety finding — CONFIRMED**

Ordinary Rod wear/Mending and other area/gear conditions require further terminal
composition. A direct Enchant Book result needs specialized item-level and book
identity settlement rather than a non-book catch-lot stack asset.

**Final decision**

Reject this selected-branch staging path unless the locked Rod is Starter Basic,
the target area is Starter Pool and no bait/enchant modifiers are active. If the
selected Treasure result is a direct Enchant Book, stop without staged output or XP
pending the dedicated book mutation implementation; on rollback all previous
preflight writes vanish. Even in the supported baseline, final operation result,
audit/outbox and external `/fish` remain prohibited until the full cast is safe.


## Fishing unmodified Rod-wear staging

Branch: feat/fishing-unmodified-rod-wear

### 1. One physical Rod-durability consequence is derived from the actual pinned branch

**Normative alignment — CONFIRMED**

A completed manual Fishing cast attempt has one ordinary Rod durability event regardless
of the number of fish or Treasure payloads. When a Fish candidate's canonical over-cap
line-break roll breaks the line, its Rod is destroyed and that consequence ignores
Unbreaking/Mending. The separate Starter Basic Rod remains unbreakable at Starter Pool.

**Final decision**

For a locked, zero-modifier manual cast, derive the actual CatchBranch from the pinned
operation RNG root. A Fish branch may require the prior versioned LineBreak PPM check:
break means `LineBreak` durability destruction; otherwise use one normal completed
attempt wear event, including non-Fish branches and a survived (possibly escaping)
over-cap Fish. An unmodified ordinary Rod has no Unbreaking proc; never accept a
user- or adapter-supplied avoidance flag. Delegate state mutation to the existing
locked Rod durability owner and preserve the full PostgreSQL transaction boundary.

### 2. Pending-owner replay cache is not durable post-commit idempotency

**Safety finding — CONFIRMED**

The present low-level Rod durability writer is caller-transaction composable but is not
a durable operation-keyed final cast wrapper. Calling it twice inside the same pending
cast would otherwise deduct two wear events. Retrying a completed cast after process
restart requires a persisted result/mutation-key boundary, not only memory state.

**Final decision**

The non-committable pending cast caches the first staged Rod durability receipt and
returns it on repeated method calls. Rollback restores both durability and Broken
state, as well as any first-area unlock. The final cast owner MUST provide persisted
operation-level replay/finalization before any transaction commit or public `/fish`
command can expose Rod wear. A line-break terminal path must grant no successful
Fish XP, and Mending/progression ordering remains a separate unfinished composition.


## Fishing operation-keyed Rod durability replay ledger

Branch: feat/fishing-rod-wear-replay-ledger

### 1. Persisted operation-level replay prevents repeated Rod damage

**Implementation finding — CONFIRMED**

The existing authoritative Rod durability writer locks the operation, active player and
equipped instance and composes changes inside a caller-owned PostgreSQL transaction,
but it previously had no operation-keyed receipt and could not safely replay after
a completed operation or a later Rod repair.

**Final decision**

Wrap the same authoritative state writer with a unique
`(operation_id, mutation_key='fishing:rod:durability')` asset event. Lock the operation
row first, then check a pinned versioned receipt: exact area, expected current durability,
resolved consequence and player must match, including on COMMITTED retry. A mismatch or
malformed stored event fails closed. Only if no receipt exists and the operation remains
PENDING may the Rod be mutated, and the receipt is inserted in the same transaction.
This applies to ordinary wear, line-break destruction and Starter Basic no-op events.

### 2. Durable child event is not terminal cast finalization

**Safety finding — CONFIRMED**

Persisted receipt replay is necessary for idempotency but does not prove the branch
and all other effects were correctly chosen. Its outer transaction still owns the
responsibility to resolve the outcome, XP, Mending, final result, audit and outbox.

**Final decision**

Switch the pending cast's unmodified Rod staging to the operation-keyed child and
keep its local receipt cache as a same-transaction optimization. If anything fails,
the owning transaction must roll back both the Rod mutation and the asset event.
Do not activate external `/fish` or claim full lifecycle completion because the
durability child now supports persisted retries.


## Fishing selected unmodified cast consequences

Branch: feat/fishing-unmodified-selected-cast

### 1. Physical terminal stage determines which children may be provisionally staged

**Normative alignment — CONFIRMED**

An unmodified cast chooses its Fish/Junk/Treasure branch from the operation-pinned
canonical baseline draw. Junk and non-book Treasure are successful landed outcomes;
Fish first uses the frozen capability route, and an over-cap Fish must resolve line
break before catch/escape. A broken line destroys an ordinary Rod and awards no
successful Fishing progression.

**Final decision**

For the selected unmodified Fish branch, a within-capability or Starter Pool
guaranteed landing stages one unique FishInstance, exactly 2 Account XP, frozen
rarity AEXP and one ordinary Rod wear event (or a Starter Basic unbreakable
receipt). A line break stages only the Rod destruction child and never a Fish
or successful XP. A survived over-cap line remains pending a separate exact
rational catch/escape roll: this resolver stages no Fish, XP or wear for that
unresolved case. A selected Junk/non-book Treasure stages its canonical payload,
exact successful XP and the one ordinary Rod event. All children share the same
rollback-only transaction and same operation identities.

### 2. No premature global live cast or unauthorized rational conversion

**Safety finding — CONFIRMED**

Unmodified composition has neither approved general modifier normalization nor
the complete over-cap exact-rational-to-RNG mapping, and the total lifecycle
still lacks final result/audit/outbox and Mending composition. Direct Enchant
Book Treasure likewise needs its specialized delivery child.

**Final decision**

This provisional selected-outcome API must return an explicit OverCapCatchPending
stage rather than treating line survival as success or escape. Direct book results
continue to fail closed. The pending owner must never expose a transaction commit
or Discord `/fish` adapter until these final boundaries and all terminal effects
are completed atomically. Repeated child staging must replay the same item,
progression and operation-keyed Rod receipts without duplicate economic output.


## Fishing deterministic direct Enchant Book identity

### 1. Direct Book identity is a three-stage pinned slot draw

**Implementation finding — CONFIRMED**

The existing frozen direct-Fishing Enchant Book tables define five weighted pools
(58/24/12/4/2), catalog-derived members with explicit weights (including the
Mythic 45/30/25 split), and exact finished-level distributions respecting each
selected enchant's resulting-level ceiling. The versioned manual RNG context
already reserves independent Pool, Member and Level domains for each Treasure slot.

**Final decision**

Use those exact table weights and three semantic draw domains in order:
`EnchantBookPool {treasure_slot}`, `EnchantBookMember {treasure_slot}`, then
`EnchantBookLevel {treasure_slot}`. The resulting `(pool, enchant, level)` is
an immutable candidate preview with stable per-slot identity. Reject out-of-range
slots and empty/invalid canonical pools rather than applying guessed fallback
distribution. The unmodified pending cast can expose this preview only after its
real baseline CatchBranch and TreasureResult draws select Treasure / Enchant Book.

### 2. Book preview is not Book ItemInstance delivery

**Safety boundary — CONFIRMED**

A selected Enchant Book is not an ordinary stack-lot Fishing payload. Dedicated
versioned Book ItemInstance creation, bounded Item Bag/pending delivery, durable
mutation receipts and audit/outbox remain separate dependencies.

**Final decision**

Do not allow this pure identity resolver to mint Book assets or award progression.
The existing selected-cast staging continues to fail closed on direct Enchant
Books until the dedicated Book writer has been composed and tested under the
single cast-owned PostgreSQL transaction. `/fish` remains unavailable.


## Fishing selected direct Book pending outcome

**Implementation finding — CONFIRMED**

The selected unmodified cast already has a deterministic Book identity preview,
but its non-Fish staging child correctly rejects Book as a stack payload. Treating
this as an undifferentiated failed non-Fish draw obscures a valid RNG outcome.

**Final decision**

Return a typed `DirectBookPending { candidate }` result when the operation's
baseline CatchBranch and TreasureResult select a direct Enchant Book. Both
Starter Basic and ordinary unmodified cast owners preserve the same pinned
candidate on replay and deliberately stage no Book ItemInstance, successful
progression or Rod wear until the dedicated Book delivery writer exists.
Any unrelated non-Fish error continues to propagate, not become a Book.

This is an in-transaction, uncommittable resolver state, not a delivered or
claimable outcome. Book identity, XP and Rod consequences must be composed under
one complete operation and audit/outbox boundary before enabling `/fish`.


## Fishing Unbreaking X owner composition

Branch: feat/fishing-unbreaking-x-owner

### 1. Unbreaking X is branch-neutral but durability-active

**Normative alignment — CONFIRMED**

The frozen Level X Rod policy assigns Unbreaking exactly a 20% chance to ignore the
one ordinary durability event. It does not alter Fish/Junk/Treasure branch weights,
Fish identity, capability or line-break probability. The manual Fishing RNG owner
already reserves the independent `UnbreakingOrdinaryWear` 1,000,000-PPM domain,
and the durability policy explicitly says Unbreaking never prevents line-break
destruction.

**Final decision**

Permit a narrow baseline path only when the locked Rod is ordinary, non-Gold, has
exactly one embedded enchant, that enchant is Unbreaking at its canonical Level X,
and no bait was consumed. Reuse the canonical baseline branch/Fish/capability kernels
because Unbreaking cannot affect them. Convert the frozen exact 1/5 probability to
200,000 PPM only after proving exact divisibility by the versioned manual Fishing
PPM scale, then compare the independent pinned draw using `draw < chance`.

For a line break, do not sample or consult Unbreaking; stage the existing line-break
destruction receipt. For every other completed attempt, pass the exact prevention
result to the operation-keyed Rod durability child. Replay returns the same receipt,
and rollback removes the provisional Rod mutation/event.

### 2. Level X cannot be extrapolated downward

**Safety finding — CONFIRMED**

The scalar policy is explicitly a final-Level-X contract. It does not freeze an
interpolation formula for Levels I-IX. Gold Rod also changes Treasure branch weight,
so combining Gold with Unbreaking X would no longer be the baseline distribution.

**Final decision**

Unbreaking I-IX, Gold Rod + Unbreaking X, additional Rod enchants and active bait
all fail closed in this slice. Do not infer a per-level prevention curve or bypass
the shared modifier/cap composer. This durability composition remains rollback-only;
it does not finalize the cast, settle Mending, resolve over-cap catch/escape or
enable `/fish`.


## Fishing Unbreaking X selected outcome

Branch: feat/fishing-unbreaking-x-outcome

**Normative alignment — CONFIRMED**

Unbreaking X changes only the one ordinary durability event. It does not alter
the baseline branch table, Fish identity, capability, Treasure category or
successful-progression tables. The previous slice already proves the exact
operation-pinned 20% prevention draw and line-break override.

**Final decision**

Compose the actual selected baseline branch under the same transaction for the
exact Unbreaking-X-only loadout. Landed Fish receives its unique FishInstance,
canonical successful Account XP/Fishing AEXP and the operation-keyed durability
receipt. Junk/non-book Treasure receives its canonical payload/progression and
the same durability child. Line break stages only Rod destruction. A survived
over-cap Fish remains catch/escape-pending and a direct Book remains identity-only
until their separate unresolved owners are complete.

Same-operation repeated staging must replay all child receipts. Rollback removes
Fish/payload delivery, progression events and Rod wear together. This still grants
no transaction commit or Discord `/fish` exposure.
