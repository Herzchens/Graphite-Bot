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
