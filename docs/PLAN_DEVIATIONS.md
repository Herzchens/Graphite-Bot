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
