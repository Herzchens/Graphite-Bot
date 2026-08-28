#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandId {
    Help,
    Register,
    Tos,
    Profile,
    Balance,
    Bank,
    Transactions,
}

impl CommandId {
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        if token.eq_ignore_ascii_case("help") || token.eq_ignore_ascii_case("h") {
            Some(Self::Help)
        } else if token.eq_ignore_ascii_case("register") || token.eq_ignore_ascii_case("reg") {
            Some(Self::Register)
        } else if token.eq_ignore_ascii_case("tos") {
            Some(Self::Tos)
        } else if token.eq_ignore_ascii_case("profile") || token.eq_ignore_ascii_case("p") {
            Some(Self::Profile)
        } else if token.eq_ignore_ascii_case("balance")
            || token.eq_ignore_ascii_case("bal")
            || token.eq_ignore_ascii_case("wallet")
        {
            Some(Self::Balance)
        } else if token.eq_ignore_ascii_case("bank") || token.eq_ignore_ascii_case("bk") {
            Some(Self::Bank)
        } else if token.eq_ignore_ascii_case("transactions") || token.eq_ignore_ascii_case("tx") {
            Some(Self::Transactions)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParsedTextCommand<'a> {
    pub id: CommandId,
    pub args: &'a str,
}

#[must_use]
pub fn parse_text_command<'a>(
    input: &'a str,
    bot_user_id: u64,
    guild_prefix: Option<&str>,
) -> Option<ParsedTextCommand<'a>> {
    let input = input.trim_start();

    if let Some(rest) = strip_mention_prefix(input, bot_user_id) {
        return parse_tail(rest);
    }

    let mut best: Option<(usize, &'a str)> = None;
    for prefix in [Some("graphite"), guild_prefix, Some("g")]
        .into_iter()
        .flatten()
    {
        if prefix.is_empty() || input.len() < prefix.len() {
            continue;
        }
        let (head, rest) = input.split_at(prefix.len());
        if head.eq_ignore_ascii_case(prefix) && best.is_none_or(|(length, _)| prefix.len() > length)
        {
            best = Some((prefix.len(), rest));
        }
    }

    best.and_then(|(_, rest)| parse_tail(rest))
}

fn parse_tail(rest: &str) -> Option<ParsedTextCommand<'_>> {
    let rest = rest.trim_start();
    if rest.is_empty() {
        return None;
    }

    let token_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let token = &rest[..token_end];
    let args = rest[token_end..].trim_start();
    let id = CommandId::from_token(token)?;
    Some(ParsedTextCommand { id, args })
}

fn strip_mention_prefix(input: &str, bot_user_id: u64) -> Option<&str> {
    let inner = input.strip_prefix("<@")?;
    let inner = inner.strip_prefix('!').unwrap_or(inner);
    let close = inner.find('>')?;
    let user_id = inner[..close].parse::<u64>().ok()?;
    if user_id != bot_user_id {
        return None;
    }
    Some(&inner[(close + 1)..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_prefix_supports_joined_and_spaced_forms() {
        let cases = [
            ("gbalance", CommandId::Balance),
            ("g balance", CommandId::Balance),
            ("graphitebalance", CommandId::Balance),
            ("graphite balance", CommandId::Balance),
            ("G BAL", CommandId::Balance),
            ("g wallet", CommandId::Balance),
            ("gbank", CommandId::Bank),
            ("g bk", CommandId::Bank),
        ];

        for (input, expected) in cases {
            assert_eq!(parse_text_command(input, 42, None).unwrap().id, expected);
        }
    }

    #[test]
    fn longest_matching_prefix_wins() {
        let parsed = parse_text_command("graphite tx", 42, Some("graph")).unwrap();
        assert_eq!(parsed.id, CommandId::Transactions);
    }

    #[test]
    fn mention_prefix_accepts_both_discord_forms() {
        assert_eq!(
            parse_text_command("<@42> p", 42, None).unwrap().id,
            CommandId::Profile
        );
        assert_eq!(
            parse_text_command("<@!42>p", 42, None).unwrap().id,
            CommandId::Profile
        );
        assert!(parse_text_command("<@41> p", 42, None).is_none());
    }

    #[test]
    fn arguments_are_not_reparsed_as_command_tokens() {
        let parsed = parse_text_command("g register accept 3", 42, None).unwrap();
        assert_eq!(parsed.id, CommandId::Register);
        assert_eq!(parsed.args, "accept 3");

        let bank = parse_text_command("g bank withdraw 500", 42, None).unwrap();
        assert_eq!(bank.id, CommandId::Bank);
        assert_eq!(bank.args, "withdraw 500");
    }
}
