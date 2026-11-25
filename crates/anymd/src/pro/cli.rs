use mcp_kit::licence::{self, LicencePolicy, MoneyCheckout};

use super::license::{policy, test_policy};
use super::pricing::{format_currency_price, pre_checkout_disclosure};

pub(super) fn checkout() -> MoneyCheckout {
    MoneyCheckout::from_docs_toml(include_str!("../money_docs.toml"), "pro")
        .expect("anymd's embedded Keel Money configuration must be valid")
}

/// `anymd pro status | activate <token> | buy`; returns the exit code.
pub fn run(arguments: &[String]) -> i32 {
    if arguments.first().is_some_and(|arg| arg == "--test") {
        let arguments = &arguments[1..];
        if !matches!(
            arguments.first().map(String::as_str),
            Some("status" | "activate")
        ) {
            eprintln!("usage: anymd pro --test status | anymd pro --test activate <token>");
            return 2;
        }
        return run_with(&test_policy(), None, arguments);
    }
    let checkout = arguments
        .first()
        .filter(|command| command.as_str() == "buy")
        .map(|_| checkout());
    run_with(&policy(), checkout.as_ref(), arguments)
}

fn parse_buy_currency(arguments: &[String]) -> Result<(Option<&'static str>, Vec<String>), ()> {
    let mut currency: Option<&'static str> = None;
    let mut forwarded = Vec::with_capacity(arguments.len());
    let mut index = 0;
    while index < arguments.len() {
        let arg = arguments[index].as_str();
        if let Some(code) = arg.strip_prefix("--currency=") {
            let (canonical, _) = format_currency_price(code).ok_or(())?;
            currency = Some(canonical);
            index += 1;
            continue;
        }
        match arg {
            "--no-browser" | "--json" | "--accept-terms" => {
                forwarded.push(arguments[index].clone());
                index += 1;
            }
            "--email" => {
                let Some(next) = arguments.get(index + 1).filter(|v| !v.starts_with("--")) else {
                    return Err(());
                };
                forwarded.push(arguments[index].clone());
                forwarded.push(next.clone());
                index += 2;
            }
            "--currency" => {
                let Some(next) = arguments.get(index + 1).filter(|v| !v.starts_with("--")) else {
                    return Err(());
                };
                let (canonical, _) = format_currency_price(next).ok_or(())?;
                currency = Some(canonical);
                index += 2;
            }
            // anymd sells one licence; seat packs and quantities do not apply.
            "--pack" | "--qty" => return Err(()),
            _ => return Err(()),
        }
    }
    Ok((currency, forwarded))
}

pub(super) fn run_with(
    policy: &LicencePolicy,
    checkout: Option<&MoneyCheckout>,
    arguments: &[String],
) -> i32 {
    let mut forwarded_args: Option<Vec<String>> = None;
    let mut selected_currency: Option<&'static str> = None;
    let ok = match arguments.first().map(String::as_str) {
        Some("status") => arguments.len() == 1,
        Some("activate") => arguments.len() == 2,
        Some("buy") => match parse_buy_currency(&arguments[1..]) {
            Ok((currency, clean)) => {
                selected_currency = currency;
                let mut full = Vec::with_capacity(clean.len() + 1);
                full.push("buy".into());
                full.extend(clean);
                forwarded_args = Some(full);
                true
            }
            Err(()) => false,
        },
        _ => false,
    };
    if !ok {
        eprintln!(
            "usage: anymd pro status | anymd pro activate <token> | anymd pro buy [--currency <code>] [--email <address>] [--accept-terms] [--no-browser] [--json]"
        );
        return 2;
    }
    let dispatch_args = forwarded_args.as_deref().unwrap_or(arguments);
    match checkout {
        Some(checkout) => {
            let is_json = dispatch_args.iter().any(|a| a == "--json");
            if !is_json {
                use std::io::{IsTerminal, Write};
                let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
                let mut chosen = selected_currency.unwrap_or("USD");
                eprintln!("{}", pre_checkout_disclosure(chosen));
                if interactive
                    && selected_currency.is_none()
                    && !dispatch_args.iter().any(|a| a == "--accept-terms")
                {
                    eprint!("Currency [USD]: ");
                    let _ = std::io::stderr().flush();
                    let mut input = String::new();
                    if std::io::stdin().read_line(&mut input).is_ok() {
                        let trimmed = input.trim();
                        if !trimmed.is_empty() {
                            match format_currency_price(trimmed) {
                                Some((canonical, formatted)) => {
                                    chosen = canonical;
                                    eprintln!(
                                        "Selected price: {formatted} ({chosen}) one-time (tax included)."
                                    );
                                }
                                None => {
                                    eprintln!("anymd pro buy: unsupported currency `{trimmed}`");
                                    return 2;
                                }
                            }
                        }
                    }
                }
            }
            licence::run_money_cli(policy, checkout, dispatch_args)
        }
        None => licence::run_cli(policy, dispatch_args),
    }
}
