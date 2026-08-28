# Implementation status

This file tracks implementation against the build-order recommendation in the Graphite master specification.

| Phase | Scope | Status |
| --- | --- | --- |
| 1 | Identity / ToS / global player / PostgreSQL foundations | Implemented foundation |
| 2 | Ledger / operations / idempotency / outbox | Bank deposit/withdraw and automatic interest-accrual slices implemented with immutable ledger settlement, FIFO lots, canonical withdrawal fees, fixed-point interest remainder, idempotency, and outbox; other economy mutations pending |
| 3 | Item definitions / instances / storage / equipment | Starter-equipment slice implemented; general storage gameplay pending |
| 4 | Fixed NPC price/content registry | Pending |
| 5 | Account / Activity progression | `rebirth_count` persistence exists only to apply the canonical Bank-interest formula; progression, level, and Rebirth commands remain pending |
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

The current executable does not register slash commands for unfinished gameplay systems. Account/Activity progression and Rebirth mutation are not live even though the Bank interest engine reads the persisted Rebirth count required by the canonical formula. `/party` and `/casino` remain unavailable, and no implementation status should be interpreted as permission to activate systems the specification marks as requiring another design pass.

## Foundation invariants already enforced

- Player and operation identifiers are UUIDv7.
- A Discord user maps to at most one active global player record.
- ToS versions are immutable once a version number exists; a newer configured version can become current without rewriting history.
- Registration requires an explicit `accept=true` plus the exact current ToS version.
- Duplicate external Discord deliveries reuse the same operation result instead of repeating registration, starter issuance, Bank deposit, or Bank withdrawal.
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
- Outbox events are committed in the same PostgreSQL transaction as canonical mutation state.
- Temporary deletion cooldown identity uses a keyed HMAC fingerprint rather than storing a permanent hidden raw Discord identity tombstone.
