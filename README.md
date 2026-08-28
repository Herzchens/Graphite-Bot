# Graphite Bot

Graphite is a persistent Discord game backend implemented in Rust with PostgreSQL as the authoritative state store.

This repository is being built in the order defined by the Graphite master specification. The current implementation establishes the production foundation rather than pretending unfinished gameplay systems are live.

## Implemented foundation

- Rust workspace split into deterministic domain primitives, PostgreSQL persistence, and the Discord application.
- UUIDv7 player and operation identities.
- Versioned Terms of Service storage and explicit acceptance during registration.
- Global player identity keyed by a unique Discord snowflake.
- Idempotent operation records with immutable request hashes, persisted RNG roots, committed results, and transactional outbox events.
- Starter equipment creation and automatic equip on first registration.
- Non-negative Wallet/Bank/liability materialized balances.
- Immutable double-entry ledger schema with deferred balance enforcement.
- HMAC identity fingerprints for the temporary post-deletion re-registration cooldown.
- Deterministic domain-separated ChaCha12 RNG primitives seeded from the operating-system CSPRNG.
- Global text prefixes (`g`, `graphite`) and Discord mention parsing for the currently active command subset.
- Slash and text invocation paths share the same application handlers.

## Active command subset

Only foundation-safe commands are registered right now:

- `/help`
- `/tos`
- `/register`
- `/profile`
- `/balance`
- `/transactions`

Mining, fishing, economy mutations, services, market/trade, clan, automation, minigames, and other systems are intentionally **not exposed as completed gameplay** until their implementation slice satisfies the master specification.

## Requirements

The production baseline is deliberately pinned to stable releases that were verified on 2026-08-28:

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

The CI workflow provisions PostgreSQL 18.6 and runs the persistence integration test. CI is intentionally read-only: it reports formatting or test failures instead of committing fixes back to the PR branch.

## Architecture rule

PostgreSQL owns canonical player, asset, balance, operation, and outbox state. In-memory data may accelerate reads, but it is never the source of truth for ownership or Money. State-changing handlers must resolve a bounded mutation and settle it atomically before Discord side effects are emitted.
