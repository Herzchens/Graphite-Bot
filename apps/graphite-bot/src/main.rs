use std::{
    env,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context as _, Result, bail};
use graphite_core::{CommandId, IdentityFingerprint, parse_text_command};
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

#[derive(Clone)]
struct App {
    store: PgStore,
    identity_hmac_key: Arc<Vec<u8>>,
}

#[derive(Clone, Debug)]
enum CommandPayload {
    None,
    Register {
        accept: bool,
        tos_version: Option<i32>,
    },
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
                "Active foundation commands: /help, /tos, /register, /profile, /balance, /transactions. Text prefixes: g, graphite, or a bot mention. Unfinished gameplay systems are intentionally not registered yet.",
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
            CommandId::Transactions => self.transactions_reply(request.discord_user_id).await,
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

    async fn transactions_reply(&self, discord_user_id: u64) -> Reply {
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
}

fn internal_error(action: &str, error: StoreError) -> Reply {
    error!(%error, %action, "Graphite persistence failure");
    Reply::private(format!("Unable to {action} right now."))
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
    if id != CommandId::Register {
        return CommandPayload::None;
    }

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

fn text_payload(id: CommandId, args: &str) -> CommandPayload {
    if id != CommandId::Register {
        return CommandPayload::None;
    }

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
        CreateCommand::new("transactions").description("Show recent immutable Money ledger lines"),
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

    let app = Arc::new(App {
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
