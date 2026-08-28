# Implementation status

This file tracks implementation against the build-order recommendation in the Graphite master specification.

| Phase | Scope | Status |
| --- | --- | --- |
| 1 | Identity / ToS / global player / PostgreSQL foundations | Implemented foundation |
| 2 | Ledger / operations / idempotency / outbox | Bank deposit/withdraw mutation slice implemented with immutable ledger settlement, FIFO lots, canonical withdrawal fees, idempotency, and outbox; Bank interest and other economy mutations pending |
| 3 | Item definitions / instances / storage / equipment | Starter-equipment slice implemented; general storage gameplay pending |
| 4 | Fixed NPC price/content registry | Pending |
| 5 | Account / Activity progression | Pending |
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

The current executable does not register slash commands for unfinished gameplay systems. In particular, Bank interest accrual remains unavailable even though `/bank` balance/deposit/withdraw is active. `/party` and `/casino` remain unavailable, and no implementation status should be interpreted as permission to activate systems the specification marks as requiring another design pass.

## Foundation invariants already enforced

- Player and operation identifiers are UUIDv7.
- A Discord user maps to at most one active global player record.
- ToS versions are immutable once a version number exists; a newer configured version can become current without rewriting history.
- Registration requires an explicit `accept=true` plus the exact current ToS version.
- Duplicate external Discord deliveries reuse the same operation result instead of repeating registration, starter issuance, Bank deposit, or Bank withdrawal.
- Operation request hashes detect accidental idempotency-key reuse with different input.
- RNG root material is persisted on every operation row before future random-domain derivation is needed.
- Starter tools are account-bound and represented as unbreakable/non-repairable; starter Leather armor remains breakable/repairable.
- Wallet, Bank, and liability values cannot be negative at the database layer.
- Ledger history is immutable; a deferred trigger rejects unbalanced posting sets.
- Bank deposits create holding-age lots; withdrawals consume principal FIFO.
- Bank withdrawal fee calculation uses the canonical holding-age, pre-withdrawal balance, and rolling-24-hour marginal surcharge bands with deterministic integer ceiling.
- Bank mutation state, materialized balances, ledger postings, lot state, withdrawal audit rows, and outbox events commit atomically.
- Outbox events are committed in the same PostgreSQL transaction as canonical mutation state.
- Temporary deletion cooldown identity uses a keyed HMAC fingerprint rather than storing a permanent hidden raw Discord identity tombstone.
