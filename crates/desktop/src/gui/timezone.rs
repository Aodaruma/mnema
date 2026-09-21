use super::*;

// Existing installations keep their manual setting; new installations default to OS time.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TimezoneMode {
    Automatic,
    #[default]
    Manual,
}

pub(super) fn validate_system_timezone(name: String) -> Result<String> {
    if timezones::get_by_name(&name).is_none() {
        return Err(anyhow!(
            "OSのタイムゾーン {name} を認識できません。手動で設定してください。"
        ));
    }
    Ok(name)
}

impl MnemaGuiApp {
    pub(super) fn refresh_system_timezone(&mut self, persist: bool) {
        let result = mnema_infra::system_timezone::detect()
            .and_then(validate_system_timezone)
            .and_then(|name| self.apply_system_timezone(name, persist));
        self.timezone_error = result.err().map(|error| error.to_string());
    }

    pub(super) fn apply_system_timezone(&mut self, name: String, persist: bool) -> Result<()> {
        let name = validate_system_timezone(name)?;
        self.detected_timezone = Some(name.clone());
        if self.timezone_mode != TimezoneMode::Automatic {
            return Ok(());
        }
        let old_today = self.now_in_timezone().date().to_string();
        // The scheduling service uses persisted preferences. Update only the timezone,
        // reading the latest record so an OS change never saves unrelated form edits.
        if persist
            && self.vault.is_some()
            && self
                .scheduling_preferences
                .as_ref()
                .is_some_and(|p| p.timezone != name)
        {
            let vault = self.vault_clone()?;
            let saved = self.runtime.block_on(async {
                let repo = vault.scheduling_preferences_repo();
                let mut preferences = repo
                    .get_for_user(local_user_id())
                    .await?
                    .ok_or_else(|| anyhow!("計画設定が見つかりません"))?;
                preferences.timezone.clone_from(&name);
                preferences.updated_at = OffsetDateTime::now_utc();
                repo.upsert(preferences.clone()).await?;
                Result::<_>::Ok(preferences)
            })?;
            self.scheduling_preferences = Some(saved);
        }
        if self.timezone_offset != name {
            self.timezone_offset = name;
            if self.target_date == old_today {
                self.target_date = self.now_in_timezone().date().to_string();
            }
            self.auto_preview = None;
            self.plan = None;
            self.workspace_ui.calendar.pan = [0.0; 2];
            self.scroll_home_agenda_to_now = true;
            if self.vault.is_some() {
                self.refresh_schedule();
                self.refresh_schedule_month();
            }
        }
        Ok(())
    }

    pub(super) fn select_timezone_mode(&mut self, mode: TimezoneMode) {
        if mode == self.timezone_mode {
            return;
        }
        if self.timezone_mode == TimezoneMode::Manual {
            self.manual_timezone.clone_from(&self.timezone_offset);
        }
        self.timezone_mode = mode;
        if mode == TimezoneMode::Automatic {
            self.refresh_system_timezone(false);
        } else {
            self.timezone_offset.clone_from(&self.manual_timezone);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_configs_keep_manual_timezone_and_new_mode_round_trips() {
        let old = serde_json::json!({
            "vault_path": "vault", "storage_backend": "sqlite", "sqlite_path": "db",
            "database_url": "", "dark_mode": true, "timezone_offset": "Europe/London",
            "llm_provider": "disabled", "ollama_url": "", "openai_url": "",
            "planning_model": "", "routine_model": ""
        });
        let mut config: DesktopConfig = serde_json::from_value(old).unwrap();
        assert_eq!(config.timezone_mode, TimezoneMode::Manual);
        assert_eq!(config.timezone_offset, "Europe/London");
        config.timezone_mode = TimezoneMode::Automatic;
        config.manual_timezone = "Europe/London".into();
        let restored: DesktopConfig =
            serde_json::from_value(serde_json::to_value(config).unwrap()).unwrap();
        assert_eq!(restored.timezone_mode, TimezoneMode::Automatic);
        assert_eq!(restored.manual_timezone, "Europe/London");
    }

    #[test]
    fn os_timezone_updates_planning_without_saving_other_form_edits() {
        let (_directory, mut app, _ctx) = interaction_tests::fixture();
        let before = app.scheduling_preferences.clone().unwrap();
        app.availability_start = "11:00".into();
        app.timezone_mode = TimezoneMode::Automatic;
        app.target_date = app.now_in_timezone().date().to_string();
        app.apply_system_timezone("America/Los_Angeles".into(), true)
            .unwrap();
        assert_eq!(app.timezone_offset, "America/Los_Angeles");
        assert_eq!(app.target_date, app.now_in_timezone().date().to_string());
        let saved = app
            .runtime
            .block_on(
                app.vault
                    .as_ref()
                    .unwrap()
                    .scheduling_preferences_repo()
                    .get_for_user(local_user_id()),
            )
            .unwrap()
            .unwrap();
        let mut expected = before;
        expected.timezone = "America/Los_Angeles".into();
        expected.updated_at = saved.updated_at;
        assert_eq!(saved, expected);
        assert_eq!(app.availability_start, "11:00");
        assert!(
            app.apply_system_timezone("Invalid/Zone".into(), true)
                .is_err()
        );
        assert_eq!(app.timezone_offset, "America/Los_Angeles");
        app.select_timezone_mode(TimezoneMode::Manual);
        assert_eq!(app.timezone_offset, "Asia/Tokyo");
        app.apply_system_timezone("Europe/London".into(), true)
            .unwrap();
        assert_eq!(
            app.timezone_offset, "Asia/Tokyo",
            "manual setting ignores OS changes"
        );
    }
}
