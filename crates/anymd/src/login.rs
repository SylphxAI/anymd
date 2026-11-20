//! Native loopback sign-in. Only a signed offline licence is saved locally;
//! the browser's Auth session never leaves the web server in plaintext.
use std::{process::Command, time::Duration};

use anyhow::{Context, Result, bail};
use axum::{
    Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::sync::mpsc;
use url::Url;

const WEB_ORIGIN: &str = "https://anymd.sylphx.com";
const HELP: &str = "Usage: anymd login\nSign in through your browser and sync your Pro licence. Set ANYMD_APP_ORIGIN to use the assigned staging site.";

pub fn run(args: &[String]) -> Result<()> {
    match args {
        [] => {}
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{HELP}");
            return Ok(());
        }
        _ => bail!("{HELP}"),
    }
    let origin = origin(&std::env::var("ANYMD_APP_ORIGIN").unwrap_or_else(|_| WEB_ORIGIN.into()))?;
    let verifier = random_hex::<32>()?;
    let state = random_hex::<16>()?;
    let challenge = hex::encode(Sha256::digest(verifier.as_bytes()));
    let grant = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(receive(origin.clone(), state, &challenge))?;
    let result = redeem(&origin, &grant, &verifier)?;
    match result.plan.as_str() {
        "pro" => {
            let token = result.tokens.iter().find(|token| crate::pro::verify_token(token).is_ok())
                .context("Signed in, but no valid offline Pro licence was returned. Link your purchase in your dashboard and try again.")?;
            let path = crate::pro::token_path()
                .context("Could not locate the anymd configuration directory.")?;
            mcp_kit::licence::write_token(&path, token)
                .context("Could not save your offline Pro licence.")?;
            println!("Signed in. Your offline Pro licence is ready.");
        }
        "free" => println!(
            "Signed in. This account has no active Pro access. Any existing offline licence was not changed."
        ),
        "unavailable" => println!(
            "Signed in. Billing isn’t connected in this environment, so no licence was changed."
        ),
        _ => bail!("The sign-in server returned an unexpected plan."),
    }
    Ok(())
}

fn origin(raw: &str) -> Result<Url> {
    let url = Url::parse(raw)
        .map_err(|_| anyhow::anyhow!("ANYMD_APP_ORIGIN must be the anymd web origin."))?;
    let secure = url.scheme() == "https"
        && url.port().is_none()
        && url
            .host_str()
            .is_some_and(|host| host == "anymd.sylphx.com" || host.ends_with(".sylphx.app"));
    let local = url.scheme() == "http" && url.host_str() == Some("127.0.0.1");
    if !(secure || local)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!(
            "ANYMD_APP_ORIGIN must be the canonical or assigned staging origin (or HTTP on 127.0.0.1 for local development)."
        );
    }
    Ok(url)
}

fn random_hex<const N: usize>() -> Result<String> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("Could not get secure randomness for sign-in."))?;
    Ok(hex::encode(bytes))
}

#[derive(Clone)]
struct Attempt {
    state: String,
    host: String,
    tx: mpsc::Sender<String>,
}

#[derive(Deserialize)]
struct Callback {
    state: String,
    grant: String,
}

async fn callback(
    State(attempt): State<Attempt>,
    headers: HeaderMap,
    query: Result<Query<Callback>, axum::extract::rejection::QueryRejection>,
) -> impl IntoResponse {
    let accepted = query.is_ok_and(|Query(query)| {
        headers
            .get(header::HOST)
            .and_then(|host| host.to_str().ok())
            == Some(attempt.host.as_str())
            && bool::from(query.state.as_bytes().ct_eq(attempt.state.as_bytes()))
            && (40..=8192).contains(&query.grant.len())
            && query
                .grant
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            && attempt.tx.try_send(query.grant).is_ok()
    });
    let (status, text) = if accepted {
        (
            StatusCode::OK,
            "Return to your terminal to finish signing in. You can close this tab.",
        )
    } else {
        (
            StatusCode::BAD_REQUEST,
            "That sign-in response didn’t match this terminal.",
        )
    };
    (
        status,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::REFERRER_POLICY, "no-referrer"),
        ],
        text,
    )
}

async fn receive(origin: Url, state: String, challenge: &str) -> Result<String> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let mut url = origin.join("/cli-auth")?;
    url.query_pairs_mut()
        .append_pair("port", &address.port().to_string())
        .append_pair("state", &state)
        .append_pair("challenge", challenge);
    // Check availability before opening the browser or waiting at an unbound staging host.
    let status_url = origin.join("/api/auth/session")?;
    tokio::task::spawn_blocking(move || check_auth(&status_url)).await??;
    eprintln!(
        "Match this code in your browser: {}\nOpen {url}",
        &state[..8]
    );
    open_browser(url.as_str());
    let (tx, mut rx) = mpsc::channel(1);
    let app = Router::new()
        .route("/callback", get(callback))
        .with_state(Attempt {
            state,
            host: address.to_string(),
            tx,
        });
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
    });
    let result = tokio::time::timeout(Duration::from_secs(300), rx.recv()).await;
    let _ = stop.send(());
    let mut server = server;
    if tokio::time::timeout(Duration::from_secs(1), &mut server)
        .await
        .is_err()
    {
        server.abort();
    }
    result
        .ok()
        .flatten()
        .context("Sign-in timed out. Run anymd login to try again.")
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut cmd = Command::new("rundll32.exe");
        cmd.arg("url.dll,FileProtocolHandler");
        cmd
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    let _ = command
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthStatus {
    auth_configured: bool,
}

fn check_auth(url: &Url) -> Result<()> {
    let mut response = agent()
        .get(url.as_str())
        .call()
        .map_err(|_| anyhow::anyhow!("Could not reach anymd sign-in."))?;
    if !response.status().is_success() {
        bail!("Could not reach anymd sign-in.");
    }
    let status: AuthStatus = response
        .body_mut()
        .with_config()
        .limit(4096)
        .read_json()
        .map_err(|_| anyhow::anyhow!("Could not read anymd sign-in status."))?;
    if !status.auth_configured {
        bail!("Sign-in isn’t connected in this environment.");
    }
    Ok(())
}

#[derive(Deserialize)]
struct LoginResult {
    plan: String,
    tokens: Vec<String>,
}

fn redeem(origin: &Url, grant: &str, verifier: &str) -> Result<LoginResult> {
    let endpoint = origin.join("/api/cli-auth")?;
    let mut response = agent()
        .post(endpoint.as_str())
        .send_json(serde_json::json!({ "grant": grant, "verifier": verifier }))
        .map_err(|err| match err {
            ureq::Error::StatusCode(401) => {
                anyhow::anyhow!("Sign-in expired or was refused. Run anymd login to try again.")
            }
            ureq::Error::StatusCode(503) => {
                anyhow::anyhow!("Sign-in isn’t connected in this environment.")
            }
            _ => anyhow::anyhow!("Could not finish sign-in. No local licence was changed."),
        })?;
    if !response.status().is_success() {
        bail!("Could not finish sign-in. No local licence was changed.");
    }
    response
        .body_mut()
        .with_config()
        .limit(65_536)
        .read_json()
        .map_err(|_| anyhow::anyhow!("Could not read the sign-in result."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_explicit_and_never_accept_credentials_or_paths() {
        for url in [
            WEB_ORIGIN,
            "https://web-test.sylphx.app",
            "http://127.0.0.1:3000",
        ] {
            assert!(origin(url).is_ok());
        }
        for url in [
            "http://web-test.sylphx.app",
            "https://sylphx.app",
            "https://web-test.sylphx.app.example.com",
            "https://user@anymd.sylphx.com",
            "https://anymd.sylphx.com/cli-auth",
            "https://anymd.sylphx.com?profile=other",
        ] {
            assert!(origin(url).is_err(), "{url}");
        }
    }
}
