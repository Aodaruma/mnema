use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand};
use mnema_app::{
    AddHabitRequest, AutoScheduleRequest, AutoScheduleService, CaptureTaskRequest,
    CaptureTaskService, HabitService, TaskCommandService, UpdateTaskRequest,
    default_scheduling_preferences, iana_date_range,
};
use mnema_core::prelude::*;
use mnema_infra::{
    calendar::{
        CalendarError, CalendarEventDraft, CalendarReadApi, CalendarWriteApi, CredentialStore,
        EncryptedFileCredentialStore, GoogleCalendarAdapter, GoogleCalendarConfig,
        KeyringCredentialStore, OAuthPkce,
    },
    db::Vault,
};
use time::{Date, Duration, OffsetDateTime, Time, format_description};
use time_tz::{OffsetDateTimeExt, timezones};
#[cfg(test)]
use time_tz::{OffsetResult, PrimitiveDateTimeExt};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "mnema", version, about = "Mnema headless scheduling CLI")]
struct Cli {
    #[arg(
        long,
        env = "MNEMA_VAULT_PATH",
        default_value = "./vault",
        global = true
    )]
    vault: PathBuf,

    #[arg(long, env = "MNEMA_SQLITE_PATH", global = true)]
    sqlite_path: Option<PathBuf>,

    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inbox task operations.
    Task(TaskArgs),
    /// Generate or apply a schedule proposal.
    Plan(PlanArgs),
    /// Inspect saved schedule blocks.
    Schedule(ScheduleArgs),
    /// Create and manage recurring habits.
    Habit(HabitArgs),
    /// Configure named planning hours, sleep, and travel defaults.
    Hours(HoursArgs),
    /// Connect, sync, and write back Google Calendar.
    Calendar(CalendarArgs),
}

#[derive(Debug, Args)]
struct TaskArgs {
    #[command(subcommand)]
    action: TaskAction,
}

#[derive(Debug, Subcommand)]
enum TaskAction {
    Add {
        title: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        due: Option<String>,
        #[arg(long)]
        minutes: Option<u32>,
    },
    List {
        #[arg(long)]
        include_deleted: bool,
    },
    Complete {
        id: String,
    },
    Update {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        due: Option<String>,
        #[arg(long)]
        clear_due: bool,
        #[arg(long)]
        minutes: Option<u32>,
        #[arg(long)]
        clear_minutes: bool,
    },
    Delete {
        id: String,
    },
}

#[derive(Debug, Args)]
struct PlanArgs {
    #[command(subcommand)]
    action: PlanAction,
}

#[derive(Debug, Subcommand)]
enum PlanAction {
    Preview(PlanOptions),
    Apply {
        #[command(flatten)]
        options: PlanOptions,
        /// Fingerprint printed by `plan preview`.
        #[arg(long)]
        fingerprint: String,
    },
    /// Recompute a rolling proposal after constraints changed.
    Repair(PlanOptions),
}

#[derive(Debug, Clone, Args)]
struct PlanOptions {
    #[arg(long)]
    date: Option<String>,
    #[arg(long, default_value_t = 7, value_parser = clap::value_parser!(u32).range(1..=31))]
    days: u32,
    /// IANA timezone. Omit to use the saved scheduling preference.
    #[arg(long)]
    timezone: Option<String>,
    /// Named-hours policies to use; empty uses all configured policies.
    #[arg(long, value_delimiter = ',')]
    named_hours: Vec<String>,
}

#[derive(Debug, Args)]
struct ScheduleArgs {
    #[command(subcommand)]
    action: ScheduleAction,
}

#[derive(Debug, Subcommand)]
enum ScheduleAction {
    List {
        #[arg(long)]
        date: Option<String>,
        #[arg(long, default_value = "Asia/Tokyo")]
        timezone: String,
    },
}

#[derive(Debug, Args)]
struct HabitArgs {
    #[command(subcommand)]
    action: HabitAction,
}

#[derive(Debug, Subcommand)]
enum HabitAction {
    Add {
        title: String,
        #[arg(long, default_value_t = 30)]
        minutes: u32,
        /// Comma-separated weekdays. Omit for every day.
        #[arg(long, value_delimiter = ',')]
        weekdays: Vec<String>,
        #[arg(long, requires = "latest")]
        earliest: Option<String>,
        #[arg(long, requires = "earliest")]
        latest: Option<String>,
        #[arg(long)]
        required: bool,
    },
    List,
    Expand {
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
    },
    Skip {
        occurrence_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    Snooze {
        occurrence_id: String,
        /// RFC3339 timestamp.
        #[arg(long)]
        until: String,
    },
    Disable {
        habit_id: String,
    },
}

#[derive(Debug, Args)]
struct HoursArgs {
    #[command(subcommand)]
    action: HoursAction,
}

#[derive(Debug, Subcommand)]
enum HoursAction {
    Show {
        #[arg(long, default_value = "Asia/Tokyo")]
        timezone: String,
    },
    Set {
        #[arg(long, default_value = "Asia/Tokyo")]
        timezone: String,
        #[arg(long, default_value = "work")]
        name: String,
        #[arg(long, default_value = "09:00")]
        work_start: String,
        #[arg(long, default_value = "17:00")]
        work_end: String,
        #[arg(long, value_delimiter = ',', default_value = "mon,tue,wed,thu,fri")]
        work_days: Vec<String>,
        #[arg(long, default_value = "23:00")]
        sleep_start: String,
        #[arg(long, default_value = "07:00")]
        sleep_end: String,
        #[arg(
            long,
            value_delimiter = ',',
            default_value = "mon,tue,wed,thu,fri,sat,sun"
        )]
        sleep_days: Vec<String>,
        #[arg(long, default_value_t = 15)]
        travel_minutes: u32,
    },
}

#[derive(Debug, Args)]
struct CalendarArgs {
    #[command(subcommand)]
    action: CalendarAction,
}

#[derive(Debug, Subcommand)]
enum CalendarAction {
    /// Generate a Google OAuth URL and PKCE material.
    AuthUrl {
        #[arg(long)]
        read_write: bool,
        #[arg(long)]
        account_id: Option<String>,
    },
    /// Exchange an OAuth code and register the Google account.
    Exchange {
        #[arg(long)]
        account_id: String,
        #[arg(long, env = "MNEMA_GOOGLE_OAUTH_CODE", hide_env_values = true)]
        code: String,
        #[arg(long, env = "MNEMA_GOOGLE_OAUTH_VERIFIER", hide_env_values = true)]
        verifier: String,
        /// State printed by `calendar auth-url`.
        #[arg(long, env = "MNEMA_GOOGLE_OAUTH_STATE", hide_env_values = true)]
        state: String,
        /// State received on the OAuth callback URL.
        #[arg(
            long,
            env = "MNEMA_GOOGLE_OAUTH_RETURNED_STATE",
            hide_env_values = true
        )]
        returned_state: String,
        #[arg(long)]
        read_write: bool,
        #[arg(long)]
        display_name: Option<String>,
    },
    List,
    Calendars {
        account_id: String,
    },
    Select {
        account_id: String,
        #[arg(long, value_delimiter = ',', required = true)]
        calendar_ids: Vec<String>,
    },
    Sync {
        #[arg(long)]
        account_id: Option<String>,
        #[arg(long)]
        full: bool,
        #[arg(long, default_value_t = 30)]
        past_days: i64,
        #[arg(long, default_value_t = 365)]
        future_days: i64,
    },
    EnsureManaged {
        account_id: String,
        #[arg(long, default_value = "Mnema Schedule")]
        name: String,
    },
    Writeback {
        account_id: String,
        #[arg(long)]
        start: Option<String>,
        #[arg(long, default_value_t = 7)]
        days: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let vault = connect_vault(&cli).await?;
    vault.initialize_defaults().await?;

    match cli.command {
        Command::Task(args) => run_task(&vault, args, cli.json).await,
        Command::Plan(args) => run_plan(&vault, args, cli.json).await,
        Command::Schedule(args) => run_schedule(&vault, args, cli.json).await,
        Command::Habit(args) => run_habit(&vault, args, cli.json).await,
        Command::Hours(args) => run_hours(&vault, args, cli.json).await,
        Command::Calendar(args) => run_calendar(&vault, args, cli.json).await,
    }
}

async fn connect_vault(cli: &Cli) -> Result<Vault> {
    match &cli.sqlite_path {
        Some(path) => Vault::connect_or_init_with_sqlite_path(&cli.vault, path).await,
        None => Vault::connect_or_init(&cli.vault).await,
    }
}

async fn run_task(vault: &Vault, args: TaskArgs, json: bool) -> Result<()> {
    let tasks = vault.task_repo();
    let statuses = vault.status_repo();

    match args.action {
        TaskAction::Add {
            title,
            description,
            due,
            minutes,
        } => {
            let lists = vault.list_repo();
            let service =
                CaptureTaskService::new(tasks.as_ref(), lists.as_ref(), statuses.as_ref());
            let result = service
                .capture_inbox_task(CaptureTaskRequest {
                    title,
                    description,
                    due_date: due.as_deref().map(parse_date).transpose()?,
                    estimated_minutes: minutes,
                })
                .await?;
            print_value(json, &result.task, || {
                format!("追加しました: {} ({})", result.task.title, result.task.id.0)
            })
        }
        TaskAction::List { include_deleted } => {
            let mut values = tasks.list_all().await?;
            if !include_deleted {
                values.retain(|task| task.deleted_at.is_none());
            }
            values.sort_by(|a, b| {
                a.due_date
                    .cmp(&b.due_date)
                    .then_with(|| a.created_at.cmp(&b.created_at))
            });
            if json {
                println!("{}", serde_json::to_string_pretty(&values)?);
            } else if values.is_empty() {
                println!("タスクはありません。");
            } else {
                for task in values {
                    println!(
                        "{}\t{}\tdue={}\testimate={}m",
                        task.id.0,
                        task.title,
                        task.due_date
                            .map(|date| date.to_string())
                            .unwrap_or_else(|| "-".into()),
                        task.estimated_minutes.unwrap_or_default()
                    );
                }
            }
            Ok(())
        }
        TaskAction::Complete { id } => {
            let service = TaskCommandService::new(tasks.as_ref(), statuses.as_ref());
            let task = service.complete_task(parse_task_id(&id)?).await?;
            print_value(json, &task, || format!("完了しました: {}", task.title))
        }
        TaskAction::Update {
            id,
            title,
            due,
            clear_due,
            minutes,
            clear_minutes,
        } => {
            let task_id = parse_task_id(&id)?;
            let current = tasks
                .find(task_id.clone())
                .await?
                .ok_or_else(|| anyhow!("タスクが見つかりません: {id}"))?;
            let service = TaskCommandService::new(tasks.as_ref(), statuses.as_ref());
            let task = service
                .update_task(UpdateTaskRequest {
                    task_id,
                    title: title.unwrap_or(current.title),
                    due_date: if clear_due {
                        None
                    } else if let Some(value) = due.as_deref() {
                        Some(parse_date(value)?)
                    } else {
                        current.due_date
                    },
                    estimated_minutes: if clear_minutes {
                        None
                    } else {
                        minutes.or(current.estimated_minutes)
                    },
                })
                .await?;
            print_value(json, &task, || format!("更新しました: {}", task.title))
        }
        TaskAction::Delete { id } => {
            let service = TaskCommandService::new(tasks.as_ref(), statuses.as_ref());
            service.delete_task(parse_task_id(&id)?).await?;
            if json {
                println!("{{\"deleted\":true}}");
            } else {
                println!("削除しました。");
            }
            Ok(())
        }
    }
}

async fn run_plan(vault: &Vault, args: PlanArgs, json: bool) -> Result<()> {
    let (options, expected_fingerprint, repair) = match args.action {
        PlanAction::Preview(options) => (options, None, false),
        PlanAction::Apply {
            options,
            fingerprint,
        } => (options, Some(fingerprint), false),
        PlanAction::Repair(options) => (options, None, true),
    };
    let saved_preferences = vault
        .scheduling_preferences_repo()
        .get_for_user(local_user_id())
        .await?;
    let timezone_name = match (
        options.timezone.as_deref(),
        saved_preferences
            .as_ref()
            .map(|value| value.timezone.as_str()),
    ) {
        (Some(requested), Some(saved)) if requested != saved => {
            return Err(anyhow!(
                "--timezone {requested} は保存済み設定 {saved} と異なります。先に `mnema hours set --timezone {requested}` を実行してください"
            ));
        }
        (Some(requested), _) => requested.to_owned(),
        (None, Some(saved)) => saved.to_owned(),
        (None, None) => "Asia/Tokyo".to_owned(),
    };
    let timezone = timezone(&timezone_name)?;
    let start_date = options
        .date
        .as_deref()
        .map(parse_date)
        .transpose()?
        .unwrap_or_else(|| OffsetDateTime::now_utc().to_timezone(timezone).date());
    let end_date_exclusive = start_date
        .checked_add(Duration::days(i64::from(options.days)))
        .ok_or_else(|| anyhow!("計画範囲が日付上限を超えています"))?;
    ensure_scheduling_preferences(vault, &timezone_name).await?;
    let tasks = vault.task_repo();
    let statuses = vault.status_repo();
    let schedule_blocks = vault.schedule_block_repo();
    let external_events = vault.external_event_repo();
    let habits = vault.habit_repo();
    let occurrences = vault.habit_occurrence_repo();
    let preferences = vault.scheduling_preferences_repo();
    let service = AutoScheduleService::new(
        tasks.as_ref(),
        statuses.as_ref(),
        schedule_blocks.as_ref(),
        external_events.as_ref(),
        habits.as_ref(),
        occurrences.as_ref(),
        preferences.as_ref(),
    );
    let preview = service
        .preview(AutoScheduleRequest {
            user_id: local_user_id(),
            start_date,
            end_date_exclusive,
            timezone: timezone_name.clone(),
            named_hours: options.named_hours.clone(),
        })
        .await?;

    if let Some(fingerprint) = expected_fingerprint {
        let applied = service.apply(&preview, &fingerprint).await?;
        if json {
            println!("{}", serde_json::to_string_pretty(&applied)?);
        } else {
            println!(
                "{}件を反映しました（fingerprint={}）",
                applied.blocks.len(),
                applied.fingerprint
            );
        }
        return Ok(());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&preview)?);
        return Ok(());
    }

    println!(
        "{}〜{}: {}件を{}しました（未配置 {}件、変更 {}件）",
        start_date,
        end_date_exclusive,
        preview.output.blocks.len(),
        if repair { "再提案" } else { "提案" },
        preview.output.unscheduled.len(),
        preview.diff.changed_count()
    );
    println!("fingerprint={}", preview.diff.fingerprint);
    for block in preview.output.blocks {
        println!(
            "{} {}-{}\t{}\t{}m",
            block.window.start.to_timezone(timezone).date(),
            block.window.start.to_timezone(timezone).time(),
            block.window.end.to_timezone(timezone).time(),
            block.title,
            block.required_minutes
        );
    }
    if !preview.output.issues.is_empty() {
        println!("未配置理由:");
        for issue in preview.output.issues {
            println!("  {issue:?}");
        }
    }
    let named_hours_arg = if options.named_hours.is_empty() {
        String::new()
    } else {
        format!(" --named-hours {}", options.named_hours.join(","))
    };
    println!(
        "反映するには: mnema plan apply --date {} --days {} --timezone {}{} --fingerprint {}",
        start_date, options.days, timezone_name, named_hours_arg, preview.diff.fingerprint
    );
    Ok(())
}

async fn run_schedule(vault: &Vault, args: ScheduleArgs, json: bool) -> Result<()> {
    match args.action {
        ScheduleAction::List { date, timezone: tz } => {
            let timezone = timezone(&tz)?;
            let date = date
                .as_deref()
                .map(parse_date)
                .transpose()?
                .unwrap_or_else(|| OffsetDateTime::now_utc().to_timezone(timezone).date());
            let end = date
                .next_day()
                .ok_or_else(|| anyhow!("日付上限を超えています"))?;
            let range = iana_date_range(date, end, &tz)?;
            let blocks = vault
                .schedule_block_repo()
                .list_overlapping(range.start, range.end)
                .await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&blocks)?);
            } else if blocks.is_empty() {
                println!("保存済み予定はありません。");
            } else {
                for block in blocks {
                    println!(
                        "{}\t{}-{}\t{}\t{:?}",
                        block.id.0,
                        block.start_at.to_timezone(timezone).time(),
                        block.end_at.to_timezone(timezone).time(),
                        block.title_snapshot.as_deref().unwrap_or("(untitled)"),
                        block.state
                    );
                }
            }
            Ok(())
        }
    }
}

async fn run_habit(vault: &Vault, args: HabitArgs, json: bool) -> Result<()> {
    let habits = vault.habit_repo();
    let occurrences = vault.habit_occurrence_repo();
    let service = HabitService::new(habits.as_ref(), occurrences.as_ref());

    match args.action {
        HabitAction::Add {
            title,
            minutes,
            weekdays,
            earliest,
            latest,
            required,
        } => {
            let schedule = if weekdays.is_empty() {
                HabitSchedule::Daily
            } else {
                HabitSchedule::Weekdays {
                    weekdays: parse_days(&weekdays)?,
                }
            };
            let preferred_window = match (earliest, latest) {
                (Some(start), Some(end)) => Some(
                    DailyTimeRange::new(parse_time(&start)?, parse_time(&end)?)
                        .map_err(|error| anyhow!(error.to_string()))?,
                ),
                (None, None) => None,
                _ => return Err(anyhow!("--earliest と --latest は両方指定してください")),
            };
            let habit = service
                .add(AddHabitRequest {
                    user_id: local_user_id(),
                    title,
                    schedule,
                    duration_minutes: minutes,
                    preferred_window,
                    flexibility: if required {
                        HabitFlexibility::Required
                    } else {
                        HabitFlexibility::Flexible
                    },
                })
                .await?;
            print_value(json, &habit, || {
                format!("Habitを追加しました: {} ({})", habit.title, habit.id.0)
            })
        }
        HabitAction::List => {
            let values = service.list(local_user_id()).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&values)?);
            } else if values.is_empty() {
                println!("Habitはありません。");
            } else {
                for habit in values {
                    println!(
                        "{}\t{}\t{}m\t{:?}\t{:?}",
                        habit.id.0,
                        habit.title,
                        habit.duration_minutes,
                        habit.flexibility,
                        habit.schedule
                    );
                }
            }
            Ok(())
        }
        HabitAction::Expand { start, end } => {
            let values = service
                .expand_occurrences(local_user_id(), parse_date(&start)?, parse_date(&end)?)
                .await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&values)?);
            } else {
                println!("{}件のOccurrenceを確認・生成しました。", values.len());
                for occurrence in values {
                    println!(
                        "{}\t{}\t{:?}",
                        occurrence.id.0, occurrence.occurrence_date, occurrence.state
                    );
                }
            }
            Ok(())
        }
        HabitAction::Skip {
            occurrence_id,
            reason,
        } => {
            let value = service
                .skip(parse_habit_occurrence_id(&occurrence_id)?, reason)
                .await?;
            print_value(json, &value, || {
                format!("{} をskipしました。", value.occurrence_date)
            })
        }
        HabitAction::Snooze {
            occurrence_id,
            until,
        } => {
            let value = service
                .snooze(
                    parse_habit_occurrence_id(&occurrence_id)?,
                    parse_timestamp(&until)?,
                )
                .await?;
            print_value(json, &value, || {
                format!("{} をsnoozeしました。", value.occurrence_date)
            })
        }
        HabitAction::Disable { habit_id } => {
            let value = service.disable(parse_habit_id(&habit_id)?).await?;
            print_value(json, &value, || {
                format!("Habitを無効化しました: {}", value.title)
            })
        }
    }
}

async fn run_hours(vault: &Vault, args: HoursArgs, json: bool) -> Result<()> {
    let repo = vault.scheduling_preferences_repo();
    match args.action {
        HoursAction::Show { timezone } => {
            let preferences = ensure_scheduling_preferences(vault, &timezone).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&preferences)?);
            } else {
                println!("timezone={}", preferences.timezone);
                for policy in &preferences.named_hours {
                    println!("hours={} hard={}", policy.name, policy.hard);
                    print_policy(policy);
                }
                if let Some(sleep) = &preferences.sleep {
                    println!("sleep={}", sleep.name);
                    print_policy(sleep);
                }
                println!(
                    "default_travel_buffer={}m",
                    preferences.default_travel_buffer_minutes
                );
            }
            Ok(())
        }
        HoursAction::Set {
            timezone: timezone_name,
            name,
            work_start,
            work_end,
            work_days,
            sleep_start,
            sleep_end,
            sleep_days,
            travel_minutes,
        } => {
            timezone(&timezone_name)?;
            let now = OffsetDateTime::now_utc();
            let existing = repo.get_for_user(local_user_id()).await?;
            let preferences = SchedulingPreferences {
                id: existing
                    .as_ref()
                    .map(|value| value.id.clone())
                    .unwrap_or_else(SchedulingPolicyId::new),
                user_id: local_user_id(),
                timezone: timezone_name,
                named_hours: vec![weekly_policy(
                    name,
                    false,
                    parse_days(&work_days)?,
                    parse_time(&work_start)?,
                    parse_time(&work_end)?,
                )?],
                sleep: Some(weekly_policy(
                    "sleep".into(),
                    true,
                    parse_days(&sleep_days)?,
                    parse_time(&sleep_start)?,
                    parse_time(&sleep_end)?,
                )?),
                default_travel_buffer_minutes: travel_minutes,
                created_at: existing.as_ref().map_or(now, |value| value.created_at),
                updated_at: now,
            };
            preferences
                .validate()
                .map_err(|error| anyhow!(error.to_string()))?;
            repo.upsert(preferences.clone()).await?;
            print_value(json, &preferences, || {
                "Hours / Sleep / Travel設定を保存しました。".into()
            })
        }
    }
}

async fn run_calendar(vault: &Vault, args: CalendarArgs, json_output: bool) -> Result<()> {
    let account_repo = vault.calendar_account_repo();
    match args.action {
        CalendarAction::AuthUrl {
            read_write,
            account_id,
        } => {
            let (adapter, _) = google_adapter(vault)?;
            let account_id = account_id
                .as_deref()
                .map(parse_calendar_account_id)
                .transpose()?
                .unwrap_or_else(CalendarAccountId::new);
            let pkce = OAuthPkce::generate();
            let access_mode = calendar_access_mode(read_write);
            let url =
                adapter.authorization_url(pkce.state(), access_mode, pkce.code_challenge())?;
            let value = serde_json::json!({
                "account_id": account_id.0,
                "state": pkce.state(),
                "code_verifier": pkce.code_verifier(),
                "authorization_url": url.as_str(),
                "access_mode": access_mode,
            });
            if json_output {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!("Google認証URL:\n{url}");
                println!("account_id={}", account_id.0);
                println!("state={}", pkce.state());
                println!("code_verifier={}", pkce.code_verifier());
                println!(
                    "認証後、codeとcallback URLのstate、および上記account_id/state/code_verifierを `mnema calendar exchange` に渡してください。"
                );
            }
            Ok(())
        }
        CalendarAction::Exchange {
            account_id,
            code,
            verifier,
            state,
            returned_state,
            read_write,
            display_name,
        } => {
            let (adapter, credentials) = google_adapter(vault)?;
            let temporary_id = parse_calendar_account_id(&account_id)?;
            let access_mode = calendar_access_mode(read_write);
            let pkce = OAuthPkce::from_parts(state, verifier.clone())?;
            if !pkce.matches_state(&returned_state) {
                return Err(anyhow!(
                    "OAuth stateが一致しません。認証をやり直してください"
                ));
            }
            adapter
                .exchange_code(temporary_id.clone(), &code, &verifier)
                .await?;
            let now = OffsetDateTime::now_utc();
            let mut account = CalendarAccount {
                id: temporary_id.clone(),
                user_id: local_user_id(),
                provider: CalendarProvider::Google,
                provider_account_id: temporary_id.0.to_string(),
                display_name: display_name.unwrap_or_else(|| "Google Calendar".into()),
                email: None,
                access_mode,
                enabled: true,
                selected_calendar_ids: Vec::new(),
                managed_calendar_id: None,
                timezone: None,
                created_at: now,
                updated_at: now,
            };
            let calendars = adapter.list_calendars(&account).await?;
            let primary = calendars
                .iter()
                .find(|calendar| calendar.primary)
                .or_else(|| calendars.first())
                .ok_or_else(|| anyhow!("Google Calendarが1件も見つかりません"))?;
            account.provider_account_id = primary.id.clone();
            account.email = primary.id.contains('@').then(|| primary.id.clone());
            account.display_name = if account.display_name == "Google Calendar" {
                primary.summary.clone()
            } else {
                account.display_name
            };
            account.selected_calendar_ids = vec![primary.id.clone()];
            account.timezone = primary.timezone.clone();

            if let Some(existing) = account_repo
                .find_by_provider_identity(CalendarProvider::Google, primary.id.clone())
                .await?
            {
                if existing.id != temporary_id
                    && let Some(credential) = credentials.load(temporary_id.clone()).await?
                {
                    credentials.save(existing.id.clone(), credential).await?;
                    credentials.delete(temporary_id).await?;
                }
                account.id = existing.id;
                account.created_at = existing.created_at;
                account.managed_calendar_id = existing.managed_calendar_id;
                if !existing.selected_calendar_ids.is_empty() {
                    account.selected_calendar_ids = existing.selected_calendar_ids;
                }
            }
            account_repo.upsert(account.clone()).await?;
            print_value(json_output, &account, || {
                format!("Google Calendarを接続しました: {}", account.display_name)
            })
        }
        CalendarAction::List => {
            let accounts = account_repo.list_enabled(local_user_id()).await?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&accounts)?);
            } else if accounts.is_empty() {
                println!("接続済みカレンダーはありません。");
            } else {
                for account in accounts {
                    println!(
                        "{}\t{}\t{:?}\tselected={}\tmanaged={}",
                        account.id.0,
                        account.display_name,
                        account.access_mode,
                        account.selected_calendar_ids.join(","),
                        account.managed_calendar_id.as_deref().unwrap_or("-")
                    );
                }
            }
            Ok(())
        }
        CalendarAction::Calendars { account_id } => {
            let (adapter, _) = google_adapter(vault)?;
            let account = find_calendar_account(vault, &account_id).await?;
            let calendars = adapter.list_calendars(&account).await?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&calendars)?);
            } else {
                for calendar in calendars {
                    println!(
                        "{}\t{}\tprimary={}\tselected={}\trole={}",
                        calendar.id,
                        calendar.summary,
                        calendar.primary,
                        account.selected_calendar_ids.contains(&calendar.id),
                        calendar.access_role.as_deref().unwrap_or("-")
                    );
                }
            }
            Ok(())
        }
        CalendarAction::Select {
            account_id,
            mut calendar_ids,
        } => {
            let mut account = find_calendar_account(vault, &account_id).await?;
            if let Some(managed) = &account.managed_calendar_id {
                calendar_ids.retain(|calendar_id| calendar_id != managed);
            }
            account.selected_calendar_ids = calendar_ids;
            account.updated_at = OffsetDateTime::now_utc();
            account_repo.upsert(account.clone()).await?;
            print_value(json_output, &account, || {
                "同期対象カレンダーを更新しました。".into()
            })
        }
        CalendarAction::Sync {
            account_id,
            full,
            past_days,
            future_days,
        } => {
            if past_days < 0 || future_days < 1 {
                return Err(anyhow!(
                    "--past-days は0以上、--future-days は1以上にしてください"
                ));
            }
            let (adapter, _) = google_adapter(vault)?;
            let accounts = if let Some(id) = account_id {
                vec![find_calendar_account(vault, &id).await?]
            } else {
                account_repo.list_enabled(local_user_id()).await?
            };
            let events = vault.external_event_repo();
            let cursors = vault.calendar_sync_cursor_repo();
            let now = OffsetDateTime::now_utc();
            let time_min = now - Duration::days(past_days);
            let time_max = now + Duration::days(future_days);
            let mut reports = Vec::new();
            for account in accounts {
                for calendar_id in &account.selected_calendar_ids {
                    if account.managed_calendar_id.as_deref() == Some(calendar_id.as_str()) {
                        continue;
                    }
                    let cursor = cursors.get(account.id.clone(), calendar_id.clone()).await?;
                    let (report, incremental) = if should_full_sync(full, cursor.as_ref(), now) {
                        (
                            adapter
                                .full_sync(
                                    &account,
                                    calendar_id,
                                    Some(time_min),
                                    Some(time_max),
                                    events.as_ref(),
                                    cursors.as_ref(),
                                )
                                .await?,
                            false,
                        )
                    } else {
                        match adapter
                            .incremental_sync(
                                &account,
                                calendar_id,
                                events.as_ref(),
                                cursors.as_ref(),
                            )
                            .await
                        {
                            Ok(report) => (report, true),
                            Err(
                                CalendarError::FullSyncRequired | CalendarError::SyncTokenExpired,
                            ) => (
                                adapter
                                    .full_sync(
                                        &account,
                                        calendar_id,
                                        Some(time_min),
                                        Some(time_max),
                                        events.as_ref(),
                                        cursors.as_ref(),
                                    )
                                    .await?,
                                false,
                            ),
                            Err(error) => return Err(error.into()),
                        }
                    };
                    reports.push(serde_json::json!({
                        "account_id": account.id.0,
                        "calendar_id": calendar_id,
                        "upserted": report.upserted,
                        "cancelled": report.cancelled,
                        "pages": report.pages,
                        "incremental": incremental,
                    }));
                }
            }
            if json_output {
                println!("{}", serde_json::to_string_pretty(&reports)?);
            } else {
                for report in reports {
                    println!(
                        "{}\tupserted={} cancelled={} pages={}",
                        report["calendar_id"].as_str().unwrap_or("-"),
                        report["upserted"],
                        report["cancelled"],
                        report["pages"]
                    );
                }
            }
            Ok(())
        }
        CalendarAction::EnsureManaged { account_id, name } => {
            let (adapter, _) = google_adapter(vault)?;
            let mut account = find_calendar_account(vault, &account_id).await?;
            let calendar = adapter.ensure_managed_calendar(&account, &name).await?;
            account.managed_calendar_id = Some(calendar.id.clone());
            account.updated_at = OffsetDateTime::now_utc();
            account_repo.upsert(account).await?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&calendar)?);
            } else {
                println!("Mnema専用カレンダーを確認しました: {}", calendar.id);
            }
            Ok(())
        }
        CalendarAction::Writeback {
            account_id,
            start,
            days,
        } => {
            let (adapter, _) = google_adapter(vault)?;
            let account = find_calendar_account(vault, &account_id).await?;
            let timezone_name = account.timezone.as_deref().unwrap_or("Asia/Tokyo");
            let start_date = start
                .as_deref()
                .map(parse_date)
                .transpose()?
                .unwrap_or_else(|| {
                    let timezone = timezone(timezone_name).unwrap_or(timezones::db::UTC);
                    OffsetDateTime::now_utc().to_timezone(timezone).date()
                });
            let end_date = start_date
                .checked_add(Duration::days(i64::from(days)))
                .ok_or_else(|| anyhow!("write-back範囲が日付上限を超えています"))?;
            let window = iana_date_range(start_date, end_date, timezone_name)?;
            let report = writeback_managed_blocks(vault, &adapter, account, window).await?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "write-back: created={} updated={} deleted={}",
                    report["created"], report["updated"], report["deleted"]
                );
            }
            Ok(())
        }
    }
}

fn google_adapter(vault: &Vault) -> Result<(GoogleCalendarAdapter, Arc<dyn CredentialStore>)> {
    let credentials: Arc<dyn CredentialStore> =
        if let Ok(encoded_key) = std::env::var("MNEMA_CREDENTIAL_KEY") {
            Arc::new(EncryptedFileCredentialStore::from_base64_key(
                vault.root.join(".credentials"),
                encoded_key.trim(),
            )?)
        } else if let Ok(key_file) = std::env::var("MNEMA_CREDENTIAL_KEY_FILE") {
            let encoded_key = std::fs::read_to_string(&key_file)
                .with_context(|| format!("credential key fileを読めません: {key_file}"))?;
            Arc::new(EncryptedFileCredentialStore::from_base64_key(
                vault.root.join(".credentials"),
                encoded_key.trim(),
            )?)
        } else {
            Arc::new(KeyringCredentialStore::mnema())
        };
    let adapter =
        GoogleCalendarAdapter::new(GoogleCalendarConfig::from_env()?, credentials.clone());
    Ok((adapter, credentials))
}

fn calendar_access_mode(read_write: bool) -> CalendarAccessMode {
    if read_write {
        CalendarAccessMode::ReadWrite
    } else {
        CalendarAccessMode::ReadOnly
    }
}

fn should_full_sync(force: bool, cursor: Option<&CalendarSyncCursor>, now: OffsetDateTime) -> bool {
    force
        || !cursor.is_some_and(|cursor| {
            cursor.sync_token.is_some()
                && cursor
                    .last_full_sync_at
                    .is_some_and(|last| last >= now - Duration::hours(12))
        })
}

async fn find_calendar_account(vault: &Vault, value: &str) -> Result<CalendarAccount> {
    vault
        .calendar_account_repo()
        .find(parse_calendar_account_id(value)?)
        .await?
        .ok_or_else(|| anyhow!("Calendar accountが見つかりません: {value}"))
}

async fn writeback_managed_blocks(
    vault: &Vault,
    adapter: &GoogleCalendarAdapter,
    mut account: CalendarAccount,
    window: mnema_app::TimeWindow,
) -> Result<serde_json::Value> {
    if account.access_mode != CalendarAccessMode::ReadWrite {
        return Err(anyhow!("write-backにはread-write OAuth接続が必要です"));
    }
    let account_repo = vault.calendar_account_repo();
    let managed = adapter
        .ensure_managed_calendar(&account, "Mnema Schedule")
        .await?;
    if account.managed_calendar_id.as_deref() != Some(managed.id.as_str()) {
        account.managed_calendar_id = Some(managed.id.clone());
        account.updated_at = OffsetDateTime::now_utc();
        account_repo.upsert(account.clone()).await?;
    }
    let calendar_id = managed.id;
    let block_repo = vault.schedule_block_repo();
    let link_repo = vault.managed_calendar_event_link_repo();
    let blocks = block_repo
        .list_overlapping(window.start, window.end)
        .await?;
    let blocks = blocks
        .into_iter()
        .filter(|block| {
            matches!(
                block.block_type,
                ScheduleBlockType::Task | ScheduleBlockType::Habit
            ) && !matches!(
                block.state,
                ScheduleBlockState::Cancelled | ScheduleBlockState::Missed
            )
        })
        .collect::<Vec<_>>();
    let previous_links = link_repo.list_for_account(account.id.clone()).await?;
    let previous_by_block = previous_links
        .iter()
        .map(|link| (link.schedule_block_id.clone(), link.clone()))
        .collect::<HashMap<_, _>>();
    let active_ids = blocks
        .iter()
        .map(|block| block.id.clone())
        .collect::<HashSet<_>>();
    let now = OffsetDateTime::now_utc();
    let mut created = 0_u32;
    let mut updated = 0_u32;
    let mut deleted = 0_u32;

    for block in &blocks {
        let draft = CalendarEventDraft {
            provider_event_id: None,
            title: block
                .title_snapshot
                .clone()
                .unwrap_or_else(|| "Mnema focus block".into()),
            description: Some(format!("Managed by Mnema ({})", block.id.0)),
            location: None,
            start_at: block.start_at,
            end_at: block.end_at,
            all_day: false,
            timezone: account.timezone.clone(),
        };
        let create_draft = CalendarEventDraft {
            provider_event_id: Some(managed_event_provider_id(&block.id)),
            ..draft.clone()
        };
        let (remote, link_id, link_created_at) =
            if let Some(link) = previous_by_block.get(&block.id) {
                match adapter
                    .update_event(
                        &account,
                        &link.calendar_id,
                        &link.provider_event_id,
                        link.etag.as_deref(),
                        draft.clone(),
                    )
                    .await
                {
                    Ok(remote) => {
                        updated += 1;
                        (remote, link.id.clone(), link.created_at)
                    }
                    Err(CalendarError::Provider { status: 404, .. }) => {
                        let remote = adapter
                            .create_event(&account, &calendar_id, create_draft)
                            .await?;
                        created += 1;
                        (remote, link.id.clone(), link.created_at)
                    }
                    Err(error) => return Err(error.into()),
                }
            } else {
                let remote = adapter
                    .create_event(&account, &calendar_id, create_draft)
                    .await?;
                created += 1;
                (remote, ManagedCalendarEventId::new(), now)
            };
        link_repo
            .upsert(ManagedCalendarEventLink {
                id: link_id,
                account_id: account.id.clone(),
                schedule_block_id: block.id.clone(),
                calendar_id: remote.calendar_id,
                provider_event_id: remote.provider_event_id,
                etag: remote.etag,
                created_at: link_created_at,
                updated_at: now,
            })
            .await?;
    }

    for link in previous_links {
        if active_ids.contains(&link.schedule_block_id) {
            continue;
        }
        let block = block_repo.find(link.schedule_block_id.clone()).await?;
        let should_delete = block.as_ref().is_none_or(|block| {
            (block.start_at < window.end && block.end_at > window.start)
                || matches!(
                    block.state,
                    ScheduleBlockState::Cancelled | ScheduleBlockState::Missed
                )
        });
        if !should_delete {
            continue;
        }
        match adapter
            .delete_event(
                &account,
                &link.calendar_id,
                &link.provider_event_id,
                link.etag.as_deref(),
            )
            .await
        {
            Ok(()) | Err(CalendarError::Provider { status: 404, .. }) => {}
            Err(error) => return Err(error.into()),
        }
        link_repo.delete(link.id).await?;
        deleted += 1;
    }

    Ok(serde_json::json!({
        "created": created,
        "updated": updated,
        "deleted": deleted,
    }))
}

fn managed_event_provider_id(block_id: &ScheduleBlockId) -> String {
    format!("mnema{}", block_id.0.simple()).to_ascii_lowercase()
}

async fn ensure_scheduling_preferences(
    vault: &Vault,
    timezone_name: &str,
) -> Result<SchedulingPreferences> {
    timezone(timezone_name)?;
    let repo = vault.scheduling_preferences_repo();
    if let Some(preferences) = repo.get_for_user(local_user_id()).await? {
        return Ok(preferences);
    }
    let preferences =
        default_scheduling_preferences(local_user_id(), timezone_name, OffsetDateTime::now_utc());
    repo.upsert(preferences.clone()).await?;
    Ok(preferences)
}

fn weekly_policy(
    name: String,
    hard: bool,
    weekdays: Vec<DayOfWeek>,
    start: Time,
    end: Time,
) -> Result<WeeklyTimePolicy> {
    let range = DailyTimeRange::new(start, end).map_err(|error| anyhow!(error.to_string()))?;
    Ok(WeeklyTimePolicy {
        name,
        hard,
        days: weekdays
            .into_iter()
            .map(|weekday| WeekdayTimeRanges {
                weekday,
                ranges: vec![range],
            })
            .collect(),
    })
}

fn print_policy(policy: &WeeklyTimePolicy) {
    for day in &policy.days {
        let ranges = day
            .ranges
            .iter()
            .map(|range| format!("{}-{}", range.start, range.end))
            .collect::<Vec<_>>()
            .join(",");
        println!("  {:?}: {ranges}", day.weekday);
    }
}

fn parse_task_id(value: &str) -> Result<TaskId> {
    Ok(TaskId(Uuid::parse_str(value).context("task idが不正です")?))
}

fn parse_habit_id(value: &str) -> Result<HabitId> {
    Ok(HabitId(
        Uuid::parse_str(value).context("habit idが不正です")?,
    ))
}

fn parse_habit_occurrence_id(value: &str) -> Result<HabitOccurrenceId> {
    Ok(HabitOccurrenceId(
        Uuid::parse_str(value).context("habit occurrence idが不正です")?,
    ))
}

fn parse_calendar_account_id(value: &str) -> Result<CalendarAccountId> {
    Ok(CalendarAccountId(
        Uuid::parse_str(value).context("calendar account idが不正です")?,
    ))
}

fn parse_timestamp(value: &str) -> Result<OffsetDateTime> {
    OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .with_context(|| format!("RFC3339時刻が不正です: {value}"))
}

fn parse_days(values: &[String]) -> Result<Vec<DayOfWeek>> {
    let mut result = Vec::new();
    for value in values {
        let day = match value.trim().to_ascii_lowercase().as_str() {
            "mon" | "monday" | "月" => DayOfWeek::Monday,
            "tue" | "tuesday" | "火" => DayOfWeek::Tuesday,
            "wed" | "wednesday" | "水" => DayOfWeek::Wednesday,
            "thu" | "thursday" | "木" => DayOfWeek::Thursday,
            "fri" | "friday" | "金" => DayOfWeek::Friday,
            "sat" | "saturday" | "土" => DayOfWeek::Saturday,
            "sun" | "sunday" | "日" => DayOfWeek::Sunday,
            other => return Err(anyhow!("曜日が不正です: {other}")),
        };
        if !result.contains(&day) {
            result.push(day);
        }
    }
    if result.is_empty() {
        return Err(anyhow!("曜日を1つ以上指定してください"));
    }
    Ok(result)
}

fn parse_date(value: &str) -> Result<Date> {
    let format = format_description::parse("[year]-[month]-[day]")?;
    Date::parse(value, &format).with_context(|| format!("日付が不正です: {value}"))
}

fn parse_time(value: &str) -> Result<Time> {
    let format = format_description::parse("[hour]:[minute]")?;
    Time::parse(value, &format).with_context(|| format!("時刻が不正です: {value}"))
}

fn timezone(name: &str) -> Result<&'static time_tz::Tz> {
    timezones::get_by_name(name).ok_or_else(|| anyhow!("IANA timezoneが不正です: {name}"))
}

#[cfg(test)]
fn local_datetime(
    date: Date,
    time: Time,
    timezone: &'static time_tz::Tz,
    prefer_earlier: bool,
) -> Result<OffsetDateTime> {
    match date.with_time(time).assume_timezone(timezone) {
        OffsetResult::Some(value) => Ok(value),
        OffsetResult::Ambiguous(first, second) => Ok(if prefer_earlier {
            first.min(second)
        } else {
            first.max(second)
        }),
        OffsetResult::None => Err(anyhow!(
            "timezone移行により存在しないローカル時刻です: {date} {time}"
        )),
    }
}

fn print_value<T: serde::Serialize>(
    json: bool,
    value: &T,
    human: impl FnOnce() -> String,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!("{}", human());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime, time};

    #[test]
    fn resolves_tokyo_local_time() {
        let value = local_datetime(
            date!(2026 - 08 - 15),
            time!(09:00),
            timezone("Asia/Tokyo").unwrap(),
            true,
        )
        .unwrap();
        assert_eq!(value.offset().whole_hours(), 9);
    }

    #[test]
    fn rejects_unknown_timezone() {
        assert!(timezone("Mars/Olympus").is_err());
    }

    #[test]
    fn parses_weekday_aliases_without_duplicates() {
        assert_eq!(
            parse_days(&["mon".into(), "月".into(), "fri".into()]).unwrap(),
            vec![DayOfWeek::Monday, DayOfWeek::Friday]
        );
    }

    #[test]
    fn accepts_sleep_range_crossing_midnight() {
        let policy = weekly_policy(
            "sleep".into(),
            true,
            vec![DayOfWeek::Monday],
            time!(23:00),
            time!(07:00),
        )
        .unwrap();
        assert!(policy.days[0].ranges[0].crosses_midnight());
    }

    #[test]
    fn calendar_sync_refreshes_the_rolling_horizon_periodically() {
        let now = datetime!(2026-08-15 12:00 UTC);
        let cursor = CalendarSyncCursor {
            account_id: CalendarAccountId::new(),
            calendar_id: "primary".into(),
            sync_token: Some("token".into()),
            last_full_sync_at: Some(now - Duration::hours(13)),
            last_incremental_sync_at: None,
            updated_at: now,
        };
        assert!(should_full_sync(false, Some(&cursor), now));

        let fresh = CalendarSyncCursor {
            last_full_sync_at: Some(now - Duration::hours(1)),
            ..cursor
        };
        assert!(!should_full_sync(false, Some(&fresh), now));
        assert!(should_full_sync(true, Some(&fresh), now));
    }

    #[test]
    fn managed_event_id_is_stable_and_google_compatible() {
        let block_id =
            ScheduleBlockId(Uuid::parse_str("01234567-89ab-cdef-0123-456789abcdef").unwrap());

        let provider_event_id = managed_event_provider_id(&block_id);

        assert_eq!(provider_event_id, "mnema0123456789abcdef0123456789abcdef");
        assert!((5..=1024).contains(&provider_event_id.len()));
        assert!(
            provider_event_id.chars().all(|character| {
                character.is_ascii_digit() || ('a'..='v').contains(&character)
            })
        );
        assert_eq!(provider_event_id, managed_event_provider_id(&block_id));
    }
}
