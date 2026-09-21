//! Read the OS timezone as an IANA identifier, including Windows timezone mapping.
use anyhow::{Context, Result};

pub fn detect() -> Result<String> {
    iana_time_zone::get_timezone().context("OSのタイムゾーンを取得できませんでした")
}
