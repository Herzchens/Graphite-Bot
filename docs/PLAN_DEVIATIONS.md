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
- authoritative final `EnhancedCanonicalAppraisal` resolver;
- atomic Rune/material/Money/AEXP bind settlement;
- atomic unbind settlement and no-refund provenance;
- true-death SoulBind protection integration;
- bound Netherite → Graphite promotion integration with authoritative previous/new appraisal and atomic top-up settlement;
- repository `Cargo.lock` generation and validation.

## Update rule for future slices

Append a new section when implementation materially diverges from its plan or when a challenged alternative is important enough to prevent the same question from being reopened later. Record the initial assumption, evidence classification, final decision, and why the alternative was accepted or rejected. Do not record speculative findings as confirmed defects.
