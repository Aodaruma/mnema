//! Native OAuth: system browser, PKCE, short-lived IPv4 loopback callback.
use super::{
    CalendarReadApi, CredentialStore, GoogleCalendarAdapter, GoogleCalendarConfig, OAuthPkce,
};
use crate::db::Vault;
use anyhow::{Result, anyhow, bail};
use mnema_core::prelude::*;
use reqwest::Url;
use std::sync::Arc;
use time::OffsetDateTime;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub fn desktop_google_config() -> Result<GoogleCalendarConfig> {
    let client_id = std::env::var("MNEMA_GOOGLE_CLIENT_ID")
        .ok().filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("Google接続の設定が必要です。MNEMA_GOOGLE_CLIENT_ID にデスクトップ用OAuthクライアントIDを設定してください。"))?;
    Ok(GoogleCalendarConfig {
        client_id,
        client_secret: std::env::var("MNEMA_GOOGLE_CLIENT_SECRET").ok(),
        redirect_uri: "http://127.0.0.1/".into(),
        authorization_endpoint: "https://accounts.google.com/o/oauth2/v2/auth".into(),
        token_endpoint: "https://oauth2.googleapis.com/token".into(),
        api_base_url: "https://www.googleapis.com/calendar/v3/".into(),
    })
}

pub async fn connect(
    vault: &Vault,
    user_id: UserId,
    access_mode: CalendarAccessMode,
    credentials: Arc<dyn CredentialStore>,
    open_browser: impl FnOnce(String),
) -> Result<CalendarAccount> {
    let mut config = desktop_google_config()?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    config.redirect_uri = format!(
        "http://127.0.0.1:{}/oauth/callback",
        listener.local_addr()?.port()
    );
    let pkce = OAuthPkce::generate();
    let adapter = GoogleCalendarAdapter::new(config, credentials.clone());
    open_browser(
        adapter
            .authorization_url_for_pkce(&pkce, access_mode)?
            .to_string(),
    );
    let code = tokio::time::timeout(
        std::time::Duration::from_secs(180),
        receive_code(listener, &pkce),
    )
    .await
    .map_err(|_| anyhow!("Google接続がタイムアウトしました。もう一度接続してください。"))??;
    let id = CalendarAccountId::new();
    adapter
        .exchange_code(id.clone(), &code, pkce.code_verifier())
        .await?;
    let result = finish_connection(
        vault,
        &adapter,
        credentials.clone(),
        id.clone(),
        user_id,
        access_mode,
    )
    .await;
    if result.is_err() {
        // Clean up the provisional entry only; an existing account remains intact.
        if let Err(error) = credentials.delete(id).await {
            tracing::warn!(%error, "provisional Google credential cleanup failed");
        }
    }
    result
}

async fn finish_connection(
    vault: &Vault,
    adapter: &GoogleCalendarAdapter,
    credentials: Arc<dyn CredentialStore>,
    id: CalendarAccountId,
    user_id: UserId,
    access_mode: CalendarAccessMode,
) -> Result<CalendarAccount> {
    let now = OffsetDateTime::now_utc();
    let mut account = CalendarAccount {
        id: id.clone(),
        user_id,
        provider: CalendarProvider::Google,
        provider_account_id: String::new(),
        display_name: "Google Calendar".into(),
        email: None,
        access_mode,
        enabled: true,
        selected_calendar_ids: vec![],
        managed_calendar_id: None,
        timezone: None,
        created_at: now,
        updated_at: now,
    };
    let calendars = adapter.list_calendars(&account).await?;
    let primary = calendars
        .iter()
        .find(|calendar| calendar.primary)
        .ok_or_else(|| anyhow!("Googleのメインカレンダーが見つかりません。"))?;
    account.provider_account_id = primary.id.clone();
    account.email = primary.id.contains('@').then(|| primary.id.clone());
    account.display_name = primary.summary.clone();
    account.timezone = primary.timezone.clone();
    account.selected_calendar_ids = calendars
        .iter()
        .filter(|calendar| calendar.primary || calendar.selected)
        .map(|calendar| calendar.id.clone())
        .collect();
    let repo = vault.calendar_account_repo();
    if let Some(existing) = repo
        .find_by_provider_identity(CalendarProvider::Google, primary.id.clone())
        .await?
    {
        let token = credentials
            .load(id.clone())
            .await?
            .ok_or_else(|| anyhow!("Google認証情報を保存できませんでした。"))?;
        credentials.save(existing.id.clone(), token).await?;
        account.id = existing.id;
        account.created_at = existing.created_at;
        account.managed_calendar_id = existing.managed_calendar_id;
        account.selected_calendar_ids = existing.selected_calendar_ids;
        repo.upsert(account.clone()).await?;
        credentials.delete(id).await?;
    } else {
        repo.upsert(account.clone()).await?;
    }
    Ok(account)
}

fn callback_code(target: &str, pkce: &OAuthPkce) -> Result<Option<String>> {
    let url = Url::parse(&format!("http://127.0.0.1{target}"))?;
    if url.path() != "/oauth/callback" {
        return Ok(None);
    }
    let pairs = url.query_pairs().collect::<Vec<_>>();
    let value = |name: &str| {
        pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_ref())
    };
    if !value("state").is_some_and(|state| pkce.matches_state(state)) {
        return Ok(None);
    }
    if value("error").is_some() {
        bail!("Google接続がキャンセルまたは拒否されました。");
    }
    value("code")
        .filter(|code| !code.is_empty())
        .map(|code| Some(code.to_owned()))
        .ok_or_else(|| anyhow!("Google認証コードがありません。"))
}

async fn receive_code(listener: TcpListener, pkce: &OAuthPkce) -> Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut request = Vec::new();
        let read = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut bytes = [0_u8; 1024];
            while request.len() < 8192 && !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = stream.read(&mut bytes).await?;
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&bytes[..count]);
            }
            std::io::Result::Ok(())
        })
        .await;
        if !matches!(read, Ok(Ok(()))) {
            continue;
        }
        let request = String::from_utf8_lossy(&request);
        let mut line = request
            .lines()
            .next()
            .unwrap_or_default()
            .split_whitespace();
        if line.next() != Some("GET") {
            continue;
        }
        let result = callback_code(line.next().unwrap_or_default(), pkce);
        let accepted = !matches!(result, Ok(None));
        let (status, body) = if accepted {
            (
                "200 OK",
                "<!doctype html><meta charset=utf-8><title>Mnema</title><p>認証結果を受け取りました。Mnemaに戻って接続状態をご確認ください。</p>",
            )
        } else {
            ("400 Bad Request", "Invalid callback")
        };
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.write_all(reply.as_bytes()),
        )
        .await;
        match result? {
            Some(code) => return Ok(code),
            None => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_requires_exact_path_and_state() {
        let pkce = OAuthPkce::generate();
        assert!(callback_code("/favicon.ico", &pkce).unwrap().is_none());
        assert!(
            callback_code("/oauth/callback?state=wrong&code=secret", &pkce)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            callback_code(
                &format!("/oauth/callback?state={}&code=ok", pkce.state()),
                &pkce
            )
            .unwrap(),
            Some("ok".into())
        );
        assert!(
            callback_code(
                &format!("/oauth/callback?state={}&error=access_denied", pkce.state()),
                &pkce
            )
            .is_err()
        );
    }
}
