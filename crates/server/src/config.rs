use std::{env, net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use time::{Date, Time, UtcOffset, format_description::FormatItem, macros::format_description};
use time_tz::timezones;

const DATE_FORMAT: &[FormatItem<'static>] = format_description!("[year]-[month]-[day]");
const CLOCK_FORMAT: &[FormatItem<'static>] = format_description!("[hour]:[minute]");

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_addr: SocketAddr,
    pub vault_path: PathBuf,
    pub timezone: String,
    pub timezone_offset: String,
    pub planning_start: String,
    pub planning_end: String,
    pub refresh_seconds: u64,
    pub automation_mode: AutomationMode,
    pub automation_interval_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationMode {
    Off,
    Suggest,
    AutoSilent,
}

impl AutomationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Suggest => "suggest",
            Self::AutoSilent => "auto_silent",
        }
    }
}

impl ServerConfig {
    pub fn from_env() -> Result<Self> {
        let bind_addr = env_value("MNEMA_SERVER_ADDR", "127.0.0.1:8080")
            .parse::<SocketAddr>()
            .context("MNEMA_SERVER_ADDR must be an IP:PORT value")?;
        let legacy_timezone_offset = env::var("MNEMA_TIMEZONE_OFFSET")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let timezone_offset = legacy_timezone_offset
            .clone()
            .unwrap_or_else(|| "+09:00".to_string());
        let timezone = env::var("MNEMA_TIMEZONE")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                legacy_timezone_offset
                    .as_deref()
                    .and_then(legacy_offset_timezone)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "Asia/Tokyo".to_string());
        let planning_start = env_value("MNEMA_PLANNING_START", "09:00");
        let planning_end = env_value("MNEMA_PLANNING_END", "17:00");
        let refresh_seconds = env_value("MNEMA_WEB_REFRESH_SECONDS", "30")
            .parse::<u64>()
            .context("MNEMA_WEB_REFRESH_SECONDS must be a positive integer")?;
        let automation_mode = parse_automation_mode(&env_value("MNEMA_AUTOMATION_MODE", "off"))?;
        let automation_interval_seconds = env_value("MNEMA_AUTOMATION_INTERVAL_SECONDS", "0")
            .parse::<u64>()
            .context("MNEMA_AUTOMATION_INTERVAL_SECONDS must be a non-negative integer")?;

        parse_utc_offset(&timezone_offset)?;
        validate_iana_timezone(&timezone)?;
        let start = parse_clock(&planning_start)?;
        let end = parse_clock(&planning_end)?;
        if start >= end {
            return Err(anyhow!(
                "MNEMA_PLANNING_END must be after MNEMA_PLANNING_START"
            ));
        }
        if refresh_seconds == 0 {
            return Err(anyhow!(
                "MNEMA_WEB_REFRESH_SECONDS must be greater than zero"
            ));
        }

        Ok(Self {
            bind_addr,
            vault_path: PathBuf::from(env_value("MNEMA_VAULT_PATH", "./vault")),
            timezone,
            timezone_offset,
            planning_start,
            planning_end,
            refresh_seconds,
            automation_mode,
            automation_interval_seconds,
        })
    }
}

fn legacy_offset_timezone(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_uppercase().as_str() {
        "UTC" | "Z" | "+00:00" | "-00:00" => Some("Etc/UTC"),
        "JST" | "+09:00" => Some("Asia/Tokyo"),
        _ => None,
    }
}

fn parse_automation_mode(value: &str) -> Result<AutomationMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "off" | "disabled" | "none" => Ok(AutomationMode::Off),
        "suggest" | "preview" => Ok(AutomationMode::Suggest),
        "auto_silent" | "auto-silent" => Ok(AutomationMode::AutoSilent),
        _ => Err(anyhow!(
            "MNEMA_AUTOMATION_MODE must be off, suggest, or auto_silent"
        )),
    }
}

pub fn validate_iana_timezone(value: &str) -> Result<()> {
    timezones::get_by_name(value.trim())
        .map(|_| ())
        .ok_or_else(|| anyhow!("timezone must be a valid IANA name such as Asia/Tokyo"))
}

fn env_value(key: &str, fallback: &str) -> String {
    env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

pub fn parse_date(value: &str) -> Result<Date> {
    Date::parse(value.trim(), DATE_FORMAT).map_err(|_| anyhow!("date must use YYYY-MM-DD"))
}

pub fn format_date(value: Date) -> String {
    value
        .format(DATE_FORMAT)
        .unwrap_or_else(|_| value.to_string())
}

pub fn parse_clock(value: &str) -> Result<Time> {
    Time::parse(value.trim(), CLOCK_FORMAT).map_err(|_| anyhow!("time must use HH:MM"))
}

pub fn parse_utc_offset(value: &str) -> Result<UtcOffset> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("utc") || value.eq_ignore_ascii_case("z") {
        return Ok(UtcOffset::UTC);
    }
    if value.eq_ignore_ascii_case("jst") || value.eq_ignore_ascii_case("asia/tokyo") {
        return UtcOffset::from_hms(9, 0, 0).map_err(Into::into);
    }

    let bytes = value.as_bytes();
    if bytes.len() != 6 || bytes[3] != b':' || !matches!(bytes[0], b'+' | b'-') {
        return Err(anyhow!("timezone offset must use +09:00, UTC, or JST"));
    }
    let sign = if bytes[0] == b'-' { -1_i8 } else { 1_i8 };
    let hours = value[1..3]
        .parse::<i8>()
        .map_err(|_| anyhow!("invalid timezone hour"))?;
    let minutes = value[4..6]
        .parse::<i8>()
        .map_err(|_| anyhow!("invalid timezone minute"))?;
    UtcOffset::from_hms(sign * hours, sign * minutes, 0)
        .map_err(|_| anyhow!("invalid timezone offset"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_public_date_and_time_formats() {
        assert_eq!(format_date(parse_date("2026-08-15").unwrap()), "2026-08-15");
        assert_eq!(
            parse_clock("09:30").unwrap(),
            Time::from_hms(9, 30, 0).unwrap()
        );
        assert_eq!(parse_utc_offset("JST").unwrap().whole_hours(), 9);
        assert_eq!(parse_utc_offset("-05:30").unwrap().whole_minutes(), -330);
        assert!(validate_iana_timezone("America/New_York").is_ok());
        assert_eq!(
            parse_automation_mode("auto_silent").unwrap(),
            AutomationMode::AutoSilent
        );
    }
}
