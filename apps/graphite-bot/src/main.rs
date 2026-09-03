use std::{
    env,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context as _, Result, bail};
use graphite_core::{CommandId, IdentityFingerprint, parse_text_command};
use graphite_economy::{
    BANK_BONUS_PRINCIPAL_TRANCHE, BANK_MIN_WITHDRAWAL, BankError, BankInterestError,
    BankInterestService, BankService, WalletSpendError,
};
use graphite_items::{ItemError, ItemService, ItemView};
use graphite_services::{
    OrdinarySoulBindUnbindPreflightError, SoulBindUnbindLifecycleError, SoulBindUnbindService,
};
use graphite_store::{PgStore, StoreError, TosDocument};
use serenity::{
    Client,
    all::{
        Command as DiscordCommand, CommandDataOptionValue, CommandOptionType, Context,
        EventHandler, GatewayIntents, GuildId, Interaction, Message, Ready,
    },
    builder::{
        CreateCommand, CreateCommandOption, CreateInteractionResponse,
        CreateInteractionResponseMessage,
    },
};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

const BANK_INTEREST_SCAN_INTERVAL: Duration = Duration::from_secs(300);
const BANK_INTEREST_BATCH_SIZE: u32 = 250;
const BANK_INTEREST_MAX_BATCHES_PER_TICK: u8 = 8;
const DISPLAY_ITEM_LIMIT: usize = 10;

#[derive(Clone)]
struct App {
    store: PgStore,
    bank: BankService,
    bank_interest: BankInterestService,
    items: ItemService,
    soulbind_unbind: SoulBindUnbindService,
    identity_hmac_key: Arc<Vec<u8>>,
}

#[derive(Clone, Debug)]
enum BankRequest {
    Info,
    Deposit(i64),
    Withdraw(i64),
    Invalid,
}

#[derive(Clone, Debug)]
enum CommandPayload {
    None,
    Register {
        accept: bool,
        tos_version: Option<i32>,
    },
    Bank(BankRequest),
    ItemId(Option<Uuid>),
}

struct CommandRequest {
    id: CommandId,
    payload: CommandPayload,
    discord_user_id: u64,
    external_request_key: String,
}

struct Reply {
    content: String,
    ephemeral: bool,
}

impl Reply {
    fn private(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            ephemeral: true,
        }
    }

    fn public(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            ephemeral: false,
        }
    }
}

impl App {
    async fn execute(&self, request: CommandRequest) -> Reply {
        match request.id {
            CommandId::Help => Reply::private(
                "Active commands: /help, /tos, /register, /profile, /balance, /bank, /transactions, /itembag, /catchbag, /locker, /equipment, /equip, /unequip, /item, /unbind. Text prefixes: g, graphite, or a bot mention. Storage reads, equipment moves, and SoulBind removal are live; discard/Trash Recovery and unfinished gameplay systems remain unavailable.",
            ),
            CommandId::Tos => self.tos_reply().await,
            CommandId::Register => {
                self.register_reply(
                    request.discord_user_id,
                    request.external_request_key,
                    request.payload,
                )
                .await
            }
            CommandId::Profile => self.profile_reply(request.discord_user_id).await,
            CommandId::Balance => self.balance_reply(request.discord_user_id).await,
            CommandId::Bank => {
                self.bank_reply(
                    request.discord_user_id,
                    request.external_request_key,
                    request.payload,
                )
                .await
            }
            CommandId::Transactions => self.transactions_reply(request.discord_user_id).await,
            CommandId::ItemBag => self.item_bag_reply(request.discord_user_id).await,
            CommandId::CatchBag => self.catch_bag_reply(request.discord_user_id).await,
            CommandId::Locker => self.locker_reply(request.discord_user_id).await,
            CommandId::Equipment => self.equipment_reply(request.discord_user_id).await,
            CommandId::Equip => {
                self.equip_reply(
                    request.discord_user_id,
                    request.external_request_key,
                    request.payload,
                )
                .await
            }
            CommandId::Unequip => {
                self.unequip_reply(
                    request.discord_user_id,
                    request.external_request_key,
                    request.payload,
                )
                .await
            }
            CommandId::Item => {
                self.item_reply(request.discord_user_id, request.payload)
                    .await
            }
            CommandId::Unbind => {
                self.unbind_reply(
                    request.discord_user_id,
                    request.external_request_key,
                    request.payload,
                )
                .await
            }
        }
    }

    async fn tos_reply(&self) -> Reply {
        match self.store.current_tos().await {
            Ok(Some(tos)) => Reply::private(format!(
                "Current Graphite Terms of Service: v{} — {}\nSHA-256: {}",
                tos.version,
                tos.document_url,
                hex::encode(tos.document_sha256)
            )),
            Ok(None) => Reply::private(
                "No current Terms of Service has been configured by the operator. Registration is disabled until one exists.",
            ),
            Err(error) => internal_error("load Terms of Service", error),
        }
    }

    async fn register_reply(
        &self,
        discord_user_id: u64,
        external_request_key: String,
        payload: CommandPayload,
    ) -> Reply {
        let CommandPayload::Register {
            accept,
            tos_version,
        } = payload
        else {
            return Reply::private("Invalid registration request.");
        };

        if !accept {
            return match self.store.current_tos().await {
                Ok(Some(tos)) => Reply::private(format!(
                    "Read Graphite ToS v{} at {}. To register, explicitly accept that exact version with `/register accept:true tos_version:{}` or `g register accept {}`.",
                    tos.version, tos.document_url, tos.version, tos.version
                )),
                Ok(None) => Reply::private(
                    "Registration is currently disabled because no current Terms of Service is configured.",
                ),
                Err(error) => internal_error("load Terms of Service", error),
            };
        }

        let Some(tos_version) = tos_version else {
            return Reply::private(
                "Explicit acceptance requires the exact ToS version. Example: `g register accept 1`.",
            );
        };

        let fingerprint = IdentityFingerprint::for_discord_user(
            self.identity_hmac_key.as_slice(),
            discord_user_id,
        );
        match self
            .store
            .register_player(
                discord_user_id,
                tos_version,
                fingerprint.as_bytes(),
                &external_request_key,
            )
            .await
        {
            Ok(receipt) => Reply::private(format!(
                "Graphite account ready. Player `{}` accepted ToS v{}. Starter loadout: {} bound items. Operation `{}`.",
                receipt.player_id,
                receipt.tos_version,
                receipt.starter_item_count,
                receipt.operation_id
            )),
            Err(error) => Reply::private(format!("Registration rejected: {error}")),
        }
    }

    async fn profile_reply(&self, discord_user_id: u64) -> Reply {
        if let Err(reply) = self.refresh_bank_interest(discord_user_id).await {
            return reply;
        }
        match self.store.profile_for_discord(discord_user_id).await {
            Ok(Some(profile)) => Reply::public(format!(
                "Graphite profile `{}`\nCreated: {}\nStarter loadout: {}/7\nWallet: {} | Bank: {}",
                profile.player_id,
                profile.created_at,
                profile.starter_item_count,
                profile.wallet.get(),
                profile.bank.get()
            )),
            Ok(None) => {
                Reply::private("No Graphite account exists yet. Use /register after reading /tos.")
            }
            Err(error) => internal_error("load profile", error),
        }
    }

    async fn balance_reply(&self, discord_user_id: u64) -> Reply {
        if let Err(reply) = self.refresh_bank_interest(discord_user_id).await {
            return reply;
        }
        match self.store.profile_for_discord(discord_user_id).await {
            Ok(Some(profile)) => Reply::private(format!(
                "Wallet: {} Money\nBank: {} Money\nRecoverable liability: {} Money",
                profile.wallet.get(),
                profile.bank.get(),
                profile.liability.get()
            )),
            Ok(None) => {
                Reply::private("No Graphite account exists yet. Use /register after reading /tos.")
            }
            Err(error) => internal_error("load balance", error),
        }
    }

    async fn bank_reply(
        &self,
        discord_user_id: u64,
        external_request_key: String,
        payload: CommandPayload,
    ) -> Reply {
        let CommandPayload::Bank(request) = payload else {
            return Reply::private("Invalid Bank request.");
        };
        if let Err(reply) = self.refresh_bank_interest(discord_user_id).await {
            return reply;
        }

        match request {
            BankRequest::Info => match self.bank.snapshot(discord_user_id).await {
                Ok(snapshot) => Reply::private(format!(
                    "Wallet: {} Money\nBank: {} Money\nActive deposit lots: {}\nNormal minimum withdrawal: {} Money\nBase interest: 0.004%/day. Rebirth bonus applies only to the first {} Money and asymptotically raises that tranche to 0.006%/day.",
                    snapshot.wallet,
                    snapshot.bank,
                    snapshot.active_lot_count,
                    BANK_MIN_WITHDRAWAL,
                    BANK_BONUS_PRINCIPAL_TRANCHE
                )),
                Err(error) => bank_error_reply(error),
            },
            BankRequest::Deposit(amount) => match self
                .bank
                .deposit(discord_user_id, amount, &external_request_key)
                .await
            {
                Ok(receipt) => Reply::private(format!(
                    "Deposited {} Money. Wallet: {} | Bank: {}. Operation `{}`.",
                    receipt.gross_amount, receipt.wallet, receipt.bank, receipt.operation_id
                )),
                Err(error) => bank_error_reply(error),
            },
            BankRequest::Withdraw(amount) => match self
                .bank
                .withdraw(discord_user_id, amount, &external_request_key)
                .await
            {
                Ok(receipt) => Reply::private(format!(
                    "Withdrew {} Money. Fee: {} | Received: {}. Wallet: {} | Bank: {}. Operation `{}`.",
                    receipt.gross_amount,
                    receipt.fee_amount,
                    receipt.net_amount,
                    receipt.wallet,
                    receipt.bank,
                    receipt.operation_id
                )),
                Err(error) => bank_error_reply(error),
            },
            BankRequest::Invalid => Reply::private(
                "Invalid Bank syntax. Use `/bank`, `/bank deposit:<amount>`, `/bank withdraw:<amount>`, `g bank`, `g bank deposit <amount>`, or `g bank withdraw <amount>`.",
            ),
        }
    }

    async fn transactions_reply(&self, discord_user_id: u64) -> Reply {
        if let Err(reply) = self.refresh_bank_interest(discord_user_id).await {
            return reply;
        }
        match self.store.recent_transactions(discord_user_id, 10).await {
            Ok(lines) if lines.is_empty() => Reply::private("No Money ledger entries yet."),
            Ok(lines) => {
                let mut content = String::from("Recent Money ledger lines:\n");
                for line in lines {
                    use std::fmt::Write as _;
                    writeln!(
                        content,
                        "{} | {} | {:+} | {}",
                        line.transaction_id, line.kind, line.amount, line.created_at
                    )
                    .expect("writing to a String cannot fail");
                }
                Reply::private(content)
            }
            Err(error) => internal_error("load transactions", error),
        }
    }

    async fn item_bag_reply(&self, discord_user_id: u64) -> Reply {
        match self.items.item_bag(discord_user_id).await {
            Ok(snapshot) => {
                let mut content = format!(
                    "Item Bag Lv{} — {}/{} slots",
                    snapshot.level, snapshot.used_slots, snapshot.capacity_slots
                );
                if snapshot.pending_deliveries > 0 {
                    use std::fmt::Write as _;
                    write!(
                        content,
                        " — {} pending delivery",
                        snapshot.pending_deliveries
                    )
                    .expect("writing to a String cannot fail");
                }
                for stack in snapshot.stacks.iter().take(DISPLAY_ITEM_LIMIT) {
                    use std::fmt::Write as _;
                    write!(
                        content,
                        "\n{} v{} [{}] ×{} ({}/{} slots)",
                        stack.definition_key,
                        stack.definition_version,
                        stack.rarity,
                        stack.quantity,
                        stack.occupied_slots,
                        snapshot.capacity_slots
                    )
                    .expect("writing to a String cannot fail");
                }
                if snapshot.stacks.is_empty() {
                    content.push_str("\nNo stack commodities stored.");
                }
                Reply::private(content)
            }
            Err(error) => item_error_reply(error),
        }
    }

    async fn catch_bag_reply(&self, discord_user_id: u64) -> Reply {
        match self.items.catch_bag(discord_user_id).await {
            Ok(snapshot) => {
                let mut content = format!(
                    "CatchBag Lv{} — {:.3}/{:.3} kg",
                    snapshot.level,
                    snapshot.used_grams as f64 / 1000.0,
                    snapshot.capacity_grams as f64 / 1000.0
                );
                for catch in snapshot.catches.iter().take(DISPLAY_ITEM_LIMIT) {
                    use std::fmt::Write as _;
                    write!(
                        content,
                        "\n{} [{}] — {:.3} kg — `{}`",
                        catch.definition_key,
                        catch.rarity,
                        catch.weight_grams as f64 / 1000.0,
                        catch.item_instance_id
                    )
                    .expect("writing to a String cannot fail");
                }
                if snapshot.catches.is_empty() {
                    content.push_str("\nNo catches stored.");
                }
                Reply::private(content)
            }
            Err(error) => item_error_reply(error),
        }
    }

    async fn locker_reply(&self, discord_user_id: u64) -> Reply {
        match self.items.locker(discord_user_id).await {
            Ok(items) if items.is_empty() => Reply::private("Tool Locker is empty."),
            Ok(items) => {
                let mut content = String::from("Tool Locker:");
                for item in items.iter().take(DISPLAY_ITEM_LIMIT) {
                    append_item_line(&mut content, item);
                }
                Reply::private(content)
            }
            Err(error) => item_error_reply(error),
        }
    }

    async fn equipment_reply(&self, discord_user_id: u64) -> Reply {
        match self.items.equipment(discord_user_id).await {
            Ok(items) if items.is_empty() => Reply::public("No equipment is currently equipped."),
            Ok(items) => {
                let mut content = String::from("Equipped loadout:");
                for entry in items {
                    use std::fmt::Write as _;
                    write!(
                        content,
                        "\n{} — {} [{}] — `{}`{}",
                        entry.slot,
                        entry.item.definition_key,
                        entry.item.rarity,
                        entry.item.item_instance_id,
                        durability_suffix(&entry.item)
                    )
                    .expect("writing to a String cannot fail");
                }
                Reply::public(content)
            }
            Err(error) => item_error_reply(error),
        }
    }

    async fn item_reply(&self, discord_user_id: u64, payload: CommandPayload) -> Reply {
        let CommandPayload::ItemId(Some(item_id)) = payload else {
            return Reply::private("Provide a valid item UUID. Example: `g item <uuid>`.");
        };
        match self.items.item(discord_user_id, item_id).await {
            Ok(item) => Reply::private(format!(
                "Item `{}`\nDefinition: {} v{}\nCategory: {} | Rarity: {}\nLocation: {}\nStarter: {} | Bound: {} | Tradeable: {} | Sellable: {} | Discardable: {}\nEnchantable: {} | Upgradeable: {} | Unbreakable: {} | Repairable: {}{}",
                item.item_instance_id,
                item.definition_key,
                item.definition_version,
                item.category,
                item.rarity,
                item.location,
                item.is_starter,
                item.is_account_bound,
                item.is_tradeable,
                item.is_sellable,
                item.is_discardable,
                item.is_enchantable,
                item.is_upgradeable,
                item.is_unbreakable,
                item.is_repairable,
                durability_suffix(&item)
            )),
            Err(error) => item_error_reply(error),
        }
    }

    async fn equip_reply(
        &self,
        discord_user_id: u64,
        external_request_key: String,
        payload: CommandPayload,
    ) -> Reply {
        let CommandPayload::ItemId(Some(item_id)) = payload else {
            return Reply::private("Provide a valid item UUID. Example: `g equip <uuid>`.");
        };
        match self
            .items
            .equip(discord_user_id, item_id, &external_request_key)
            .await
        {
            Ok(receipt) => Reply::private(format!(
                "Equipped `{}` in {}.{} Operation `{}`.",
                item_id,
                receipt.slot.as_deref().unwrap_or("UNKNOWN"),
                receipt
                    .displaced_item_instance_id
                    .map(|id| format!(" Previous item `{id}` returned to Tool Locker."))
                    .unwrap_or_default(),
                receipt.operation_id
            )),
            Err(error) => item_error_reply(error),
        }
    }

    async fn unequip_reply(
        &self,
        discord_user_id: u64,
        external_request_key: String,
        payload: CommandPayload,
    ) -> Reply {
        let CommandPayload::ItemId(Some(item_id)) = payload else {
            return Reply::private("Provide a valid item UUID. Example: `g unequip <uuid>`.");
        };
        match self
            .items
            .unequip(discord_user_id, item_id, &external_request_key)
            .await
        {
            Ok(receipt) => Reply::private(format!(
                "Unequipped `{}` from {} to Tool Locker. Operation `{}`.",
                item_id,
                receipt.slot.as_deref().unwrap_or("UNKNOWN"),
                receipt.operation_id
            )),
            Err(error) => item_error_reply(error),
        }
    }

    async fn unbind_reply(
        &self,
        discord_user_id: u64,
        external_request_key: String,
        payload: CommandPayload,
    ) -> Reply {
        let CommandPayload::ItemId(Some(item_id)) = payload else {
            return Reply::private("Provide a valid item UUID. Example: `g unbind <uuid>`.");
        };
        match self
            .soulbind_unbind
            .unbind(discord_user_id, item_id, &external_request_key)
            .await
        {
            Ok(receipt) => Reply::private(format!(
                "SoulBind removed from `{}`. Fee: {} Money. Wallet: {}. Rebind available at {}. Operation `{}`.",
                item_id,
                receipt.money_fee,
                receipt.wallet_after,
                receipt.rebind_not_before,
                receipt.operation_id
            )),
            Err(error) => soulbind_unbind_error_reply(error),
        }
    }

    async fn refresh_bank_interest(&self, discord_user_id: u64) -> std::result::Result<(), Reply> {
        match self.bank_interest.accrue_interest(discord_user_id).await {
            Ok(_) | Err(BankInterestError::PlayerNotFound) => Ok(()),
            Err(error) => Err(bank_interest_error_reply(error)),
        }
    }
}

fn append_item_line(content: &mut String, item: &ItemView) {
    use std::fmt::Write as _;
    write!(
        content,
        "\n{} [{}] — `{}`{}",
        item.definition_key,
        item.rarity,
        item.item_instance_id,
        durability_suffix(item)
    )
    .expect("writing to a String cannot fail");
}

fn durability_suffix(item: &ItemView) -> String {
    match (item.current_durability, item.max_durability) {
        (Some(current), Some(maximum)) => format!(" — durability {current}/{maximum}"),
        _ if item.is_unbreakable => " — unbreakable".to_owned(),
        _ => String::new(),
    }
}

fn internal_error(action: &str, error: StoreError) -> Reply {
    error!(%error, %action, "Graphite persistence failure");
    Reply::private(format!("Unable to {action} right now."))
}

fn bank_error_reply(error: BankError) -> Reply {
    if matches!(
        &error,
        BankError::Database(_)
            | BankError::InvalidOperationResult(_)
            | BankError::LotIntegrityMismatch
            | BankError::OperationMissingAfterInsert
            | BankError::ArithmeticOverflow
            | BankError::InvalidFee
    ) {
        error!(%error, "Graphite Bank persistence/integrity failure");
        Reply::private("Unable to complete that Bank request right now.")
    } else {
        Reply::private(format!("Bank request rejected: {error}"))
    }
}

fn bank_interest_error_reply(error: BankInterestError) -> Reply {
    error!(%error, "Graphite Bank interest persistence/integrity failure");
    Reply::private("Unable to settle Bank interest right now.")
}

fn item_error_reply(error: ItemError) -> Reply {
    if matches!(
        &error,
        ItemError::Database(_)
            | ItemError::InvalidOperationResult(_)
            | ItemError::OperationMissingAfterInsert
            | ItemError::ArithmeticOverflow
            | ItemError::EquipmentIntegrityMismatch
    ) {
        error!(%error, "Graphite item/storage persistence/integrity failure");
        Reply::private("Unable to complete that item/storage request right now.")
    } else {
        Reply::private(format!("Item/storage request rejected: {error}"))
    }
}

fn soulbind_unbind_error_reply(error: SoulBindUnbindLifecycleError) -> Reply {
    match &error {
        SoulBindUnbindLifecycleError::PlayerNotFound => {
            Reply::private("No Graphite account exists yet. Use /register after reading /tos.")
        }
        SoulBindUnbindLifecycleError::Settlement(
            OrdinarySoulBindUnbindPreflightError::NotSoulBound,
        ) => Reply::private("That item is not currently SoulBound."),
        SoulBindUnbindLifecycleError::Settlement(
            OrdinarySoulBindUnbindPreflightError::ControlFlagsSet { .. },
        ) => Reply::private(
            "Clear both Favorite and Protected on that item before removing SoulBind.",
        ),
        SoulBindUnbindLifecycleError::Settlement(OrdinarySoulBindUnbindPreflightError::Wallet(
            WalletSpendError::InsufficientWallet {
                available,
                requested,
            },
        )) => Reply::private(format!(
            "SoulBind removal requires {requested} Money in Wallet; only {available} is available. Bank is not auto-pulled for this fee.",
        )),
        SoulBindUnbindLifecycleError::Settlement(OrdinarySoulBindUnbindPreflightError::Wallet(
            WalletSpendError::AccountFrozen(status),
        )) => Reply::private(format!(
            "SoulBind removal is unavailable while the account status is {status}.",
        )),
        _ => {
            error!(%error, "Graphite SoulBind unbind persistence/integrity failure");
            Reply::private("Unable to complete that SoulBind unbind request right now.")
        }
    }
}

struct Handler {
    app: Arc<App>,
    dev_guild_id: Option<u64>,
    bot_user_id: AtomicU64,
}

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        self.bot_user_id
            .store(ready.user.id.get(), Ordering::Relaxed);
        info!(user = %ready.user.name, "Graphite connected to Discord");
        if let Err(error) = register_commands(&ctx, self.dev_guild_id).await {
            error!(%error, "failed to register Discord application commands");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Interaction::Command(command) = interaction else {
            return;
        };

        let Some(id) = CommandId::from_token(&command.data.name) else {
            return;
        };
        let payload = slash_payload(id, &command.data.options);
        let request = CommandRequest {
            id,
            payload,
            discord_user_id: command.user.id.get(),
            external_request_key: format!("discord:interaction:{}", command.id.get()),
        };
        let reply = self.app.execute(request).await;
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content(reply.content)
                .ephemeral(reply.ephemeral),
        );
        if let Err(error) = command.create_response(&ctx.http, response).await {
            warn!(%error, "failed to respond to Discord interaction");
        }
    }

    async fn message(&self, ctx: Context, message: Message) {
        if message.author.bot {
            return;
        }

        let bot_user_id = self.bot_user_id.load(Ordering::Relaxed);
        if bot_user_id == 0 {
            return;
        }
        let Some(parsed) = parse_text_command(&message.content, bot_user_id, None) else {
            return;
        };
        let payload = text_payload(parsed.id, parsed.args);
        let request = CommandRequest {
            id: parsed.id,
            payload,
            discord_user_id: message.author.id.get(),
            external_request_key: format!("discord:message:{}", message.id.get()),
        };
        let reply = self.app.execute(request).await;
        if let Err(error) = message.reply(&ctx.http, reply.content).await {
            warn!(%error, "failed to respond to Discord text command");
        }
    }
}

fn slash_payload(id: CommandId, options: &[serenity::all::CommandDataOption]) -> CommandPayload {
    match id {
        CommandId::Register => {
            let mut accept = false;
            let mut tos_version = None;
            for option in options {
                match (option.name.as_str(), &option.value) {
                    ("accept", CommandDataOptionValue::Boolean(value)) => accept = *value,
                    ("tos_version", CommandDataOptionValue::Integer(value)) => {
                        tos_version = i32::try_from(*value).ok();
                    }
                    _ => {}
                }
            }
            CommandPayload::Register {
                accept,
                tos_version,
            }
        }
        CommandId::Bank => {
            let mut deposit = None;
            let mut withdraw = None;
            for option in options {
                match (option.name.as_str(), &option.value) {
                    ("deposit", CommandDataOptionValue::Integer(value)) => deposit = Some(*value),
                    ("withdraw", CommandDataOptionValue::Integer(value)) => {
                        withdraw = Some(*value);
                    }
                    _ => {}
                }
            }
            let request = match (deposit, withdraw) {
                (None, None) => BankRequest::Info,
                (Some(amount), None) => BankRequest::Deposit(amount),
                (None, Some(amount)) => BankRequest::Withdraw(amount),
                (Some(_), Some(_)) => BankRequest::Invalid,
            };
            CommandPayload::Bank(request)
        }
        CommandId::Equip | CommandId::Unequip | CommandId::Item | CommandId::Unbind => {
            let item_id = options.iter().find_map(|option| {
                if option.name == "item_id"
                    && let CommandDataOptionValue::String(value) = &option.value
                {
                    return Uuid::parse_str(value).ok();
                }
                None
            });
            CommandPayload::ItemId(item_id)
        }
        _ => CommandPayload::None,
    }
}

fn text_payload(id: CommandId, args: &str) -> CommandPayload {
    match id {
        CommandId::Register => {
            let mut parts = args.split_ascii_whitespace();
            let accept = parts
                .next()
                .is_some_and(|token| token.eq_ignore_ascii_case("accept"));
            let tos_version = if accept {
                parts.next().and_then(|value| value.parse::<i32>().ok())
            } else {
                None
            };
            CommandPayload::Register {
                accept,
                tos_version,
            }
        }
        CommandId::Bank => CommandPayload::Bank(parse_text_bank_request(args)),
        CommandId::Equip | CommandId::Unequip | CommandId::Item | CommandId::Unbind => {
            CommandPayload::ItemId(parse_single_uuid(args))
        }
        _ => CommandPayload::None,
    }
}

fn parse_text_bank_request(args: &str) -> BankRequest {
    let mut parts = args.split_ascii_whitespace();
    let Some(action) = parts.next() else {
        return BankRequest::Info;
    };
    let Some(amount) = parts.next().and_then(|value| value.parse::<i64>().ok()) else {
        return BankRequest::Invalid;
    };
    if parts.next().is_some() {
        return BankRequest::Invalid;
    }

    if action.eq_ignore_ascii_case("deposit") {
        BankRequest::Deposit(amount)
    } else if action.eq_ignore_ascii_case("withdraw") {
        BankRequest::Withdraw(amount)
    } else {
        BankRequest::Invalid
    }
}

fn parse_single_uuid(args: &str) -> Option<Uuid> {
    let mut parts = args.split_ascii_whitespace();
    let value = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Uuid::parse_str(value).ok()
}

fn item_id_command(name: &str, description: &str) -> CreateCommand {
    CreateCommand::new(name)
        .description(description)
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "item_id", "Item instance UUID")
                .required(true),
        )
}

async fn register_commands(ctx: &Context, dev_guild_id: Option<u64>) -> Result<()> {
    let commands = vec![
        CreateCommand::new("help").description("Show currently active Graphite commands"),
        CreateCommand::new("tos").description("View the current Graphite Terms of Service"),
        CreateCommand::new("register")
            .description("Create or re-consent a Graphite account")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::Boolean,
                    "accept",
                    "Explicitly accept the current ToS",
                )
                .required(false),
            )
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::Integer,
                    "tos_version",
                    "Exact ToS version being accepted",
                )
                .required(false),
            ),
        CreateCommand::new("profile").description("Show your public Graphite profile"),
        CreateCommand::new("balance").description("Show your Wallet and Bank balances"),
        CreateCommand::new("bank")
            .description("View Bank or move Money between Wallet and Bank")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::Integer,
                    "deposit",
                    "Move this much Money from Wallet to Bank",
                )
                .required(false),
            )
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::Integer,
                    "withdraw",
                    "Move this much Money from Bank to Wallet before fees",
                )
                .required(false),
            ),
        CreateCommand::new("transactions").description("Show recent immutable Money ledger lines"),
        CreateCommand::new("itembag").description("Show Item Bag capacity and stored stacks"),
        CreateCommand::new("catchbag").description("Show CatchBag weight and catches"),
        CreateCommand::new("locker").description("Show death-safe Tool Locker equipment"),
        CreateCommand::new("equipment").description("Show currently equipped loadout"),
        item_id_command("equip", "Equip an item instance from Tool Locker"),
        item_id_command("unequip", "Move an equipped item instance to Tool Locker"),
        item_id_command("item", "Inspect one owned item instance"),
        item_id_command(
            "unbind",
            "Remove ordinary SoulBind from an owned item instance",
        ),
    ];

    if let Some(guild_id) = dev_guild_id {
        GuildId::new(guild_id)
            .set_commands(&ctx.http, commands)
            .await?;
    } else {
        DiscordCommand::set_global_commands(&ctx.http, commands).await?;
    }
    Ok(())
}

struct Settings {
    discord_token: String,
    database_url: String,
    identity_hmac_key: Vec<u8>,
    tos_document: Option<TosDocument>,
    dev_guild_id: Option<u64>,
}

impl Settings {
    fn from_env() -> Result<Self> {
        let discord_token = env::var("DISCORD_TOKEN").context("DISCORD_TOKEN is required")?;
        let database_url = env::var("DATABASE_URL").context("DATABASE_URL is required")?;
        let identity_hmac_key = decode_hex(
            "GRAPHITE_IDENTITY_HMAC_KEY_HEX",
            &env::var("GRAPHITE_IDENTITY_HMAC_KEY_HEX")
                .context("GRAPHITE_IDENTITY_HMAC_KEY_HEX is required")?,
        )?;
        if identity_hmac_key.len() < 32 {
            bail!("GRAPHITE_IDENTITY_HMAC_KEY_HEX must decode to at least 32 bytes");
        }

        let tos_version = env::var("GRAPHITE_TOS_VERSION").ok();
        let tos_url = env::var("GRAPHITE_TOS_URL").ok();
        let tos_hash = env::var("GRAPHITE_TOS_SHA256_HEX").ok();
        let tos_document = match (tos_version, tos_url, tos_hash) {
            (None, None, None) => None,
            (Some(version), Some(document_url), Some(hash)) => Some(TosDocument {
                version: version
                    .parse()
                    .context("GRAPHITE_TOS_VERSION must be an integer")?,
                document_url,
                document_sha256: decode_hex_32("GRAPHITE_TOS_SHA256_HEX", &hash)?,
            }),
            _ => bail!(
                "GRAPHITE_TOS_VERSION, GRAPHITE_TOS_URL, and GRAPHITE_TOS_SHA256_HEX must be configured together"
            ),
        };

        let dev_guild_id = env::var("GRAPHITE_DEV_GUILD_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|value| {
                value
                    .parse::<u64>()
                    .context("GRAPHITE_DEV_GUILD_ID must be a Discord snowflake")
            })
            .transpose()?;

        Ok(Self {
            discord_token,
            database_url,
            identity_hmac_key,
            tos_document,
            dev_guild_id,
        })
    }
}

fn decode_hex(name: &str, value: &str) -> Result<Vec<u8>> {
    hex::decode(value).with_context(|| format!("{name} must contain valid hexadecimal bytes"))
}

fn decode_hex_32(name: &str, value: &str) -> Result<[u8; 32]> {
    let bytes = decode_hex(name, value)?;
    if bytes.len() != 32 {
        bail!("{name} must decode to exactly 32 bytes");
    }
    let mut output = [0_u8; 32];
    output.copy_from_slice(&bytes);
    Ok(output)
}

fn spawn_bank_interest_worker(bank_interest: BankInterestService) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(BANK_INTEREST_SCAN_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            for _ in 0..BANK_INTEREST_MAX_BATCHES_PER_TICK {
                match bank_interest
                    .accrue_due_interest_batch(BANK_INTEREST_BATCH_SIZE)
                    .await
                {
                    Ok(summary) => {
                        if summary.players_processed > 0 {
                            info!(
                                players = summary.players_processed,
                                days = summary.days_processed,
                                interest = summary.interest_credited,
                                "settled due Bank interest"
                            );
                        }
                        if summary.players_processed < BANK_INTEREST_BATCH_SIZE {
                            break;
                        }
                    }
                    Err(error) => {
                        error!(%error, "Bank interest worker batch failed");
                        break;
                    }
                }
            }
        }
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let settings = Settings::from_env()?;
    let store = PgStore::connect(&settings.database_url).await?;
    store.migrate().await?;
    if let Some(tos) = &settings.tos_document {
        store.ensure_tos_document(tos).await?;
        info!(version = tos.version, "configured current Terms of Service");
    } else {
        warn!("no current Terms of Service configured; registration will remain disabled");
    }

    let bank_interest = BankInterestService::new(store.clone());
    spawn_bank_interest_worker(bank_interest.clone());
    let app = Arc::new(App {
        bank: BankService::new(store.clone()),
        bank_interest,
        items: ItemService::new(store.clone()),
        soulbind_unbind: SoulBindUnbindService::new(store.pool().clone()),
        store,
        identity_hmac_key: Arc::new(settings.identity_hmac_key),
    });
    let handler = Handler {
        app,
        dev_guild_id: settings.dev_guild_id,
        bot_user_id: AtomicU64::new(0),
    };
    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(&settings.discord_token, intents)
        .event_handler(handler)
        .await?;
    client.start().await?;
    Ok(())
}
