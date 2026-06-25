use mnema_app::{
    AvailabilityWindow, BusyBlock, CaptureTaskRequest, CaptureTaskService, GreedyScheduler,
    PlanTodayRequest, PlanTodayService, SchedulePlanStoreService, SchedulingInput, TimeWindow,
};
use mnema_core::prelude::*;
use mnema_infra::db::Vault;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use time::{
    Date, OffsetDateTime,
    macros::{datetime, format_description},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(());
    }

    if args.iter().any(|arg| arg == "--demo-plan") {
        run_demo_plan();
        return Ok(());
    }

    match args.first().map(String::as_str) {
        Some("add") => run_add(&args[1..]).await,
        Some("list") => run_list(&args[1..]).await,
        Some("schedule") => run_schedule(&args[1..]).await,
        Some("plan") | None => run_plan(args.get(1..).unwrap_or_default()).await,
        Some(path) => {
            let path_args = vec![path.to_string()];
            run_plan(&path_args).await
        }
    }
}

async fn run_plan(args: &[String]) -> anyhow::Result<()> {
    let vault_path = vault_path_from_args(args, true);
    let save = args.iter().any(|arg| arg == "--save");

    println!("Vault: {}", vault_path.display());
    let vault = Vault::connect_or_init(&vault_path).await?;
    vault.initialize_defaults().await?;

    let target_date = OffsetDateTime::now_utc().date();
    let task_repo = vault.task_repo();
    let status_repo = vault.status_repo();
    let service = PlanTodayService::new(&task_repo, &status_repo);
    let result = service
        .plan_today(PlanTodayRequest {
            target_date,
            availability: default_workday_availability(target_date)?,
            busy_blocks: Vec::new(),
        })
        .await?;

    print_plan("Today plan", result.target_date, &result.output.blocks);
    if save {
        let schedule_block_repo = vault.schedule_block_repo();
        let store = SchedulePlanStoreService::new(&schedule_block_repo);
        let saved = store.save_proposed_plan(&result).await?;
        println!("Saved proposed schedule blocks: {}", saved.len());
    }
    if !result.output.unscheduled.is_empty() {
        println!("Unscheduled tasks: {}", result.output.unscheduled.len());
    }
    if result.output.blocks.is_empty() && result.output.unscheduled.is_empty() {
        println!("No candidate tasks for today.");
    }

    Ok(())
}

async fn run_schedule(args: &[String]) -> anyhow::Result<()> {
    let vault_path = vault_path_from_args(args, false);
    let vault = Vault::connect_or_init(&vault_path).await?;
    let target_date =
        optional_date_arg(args, "--date")?.unwrap_or_else(|| OffsetDateTime::now_utc().date());
    let schedule_block_repo = vault.schedule_block_repo();
    let store = SchedulePlanStoreService::new(&schedule_block_repo);
    let blocks = store.list_for_day(target_date).await?;

    print_saved_schedule("Saved schedule", target_date, &blocks);
    Ok(())
}

async fn run_add(args: &[String]) -> anyhow::Result<()> {
    let title = first_positional_arg(args, &["--due", "--minutes", "--vault"])
        .ok_or_else(|| anyhow::anyhow!("task title is required"))?;
    let due_date = optional_date_arg(args, "--due")?;
    let estimated_minutes = optional_u32_arg(args, "--minutes")?;
    let vault_path = vault_path_from_args(args, false);

    let vault = Vault::connect_or_init(&vault_path).await?;
    vault.initialize_defaults().await?;
    let task_repo = vault.task_repo();
    let list_repo = vault.list_repo();
    let status_repo = vault.status_repo();
    let service = CaptureTaskService::new(&task_repo, &list_repo, &status_repo);
    let result = service
        .capture_inbox_task(CaptureTaskRequest {
            title,
            description: None,
            due_date,
            estimated_minutes,
        })
        .await?;

    println!("Added task: {}", result.task.title);
    if let Some(due_date) = result.task.due_date {
        println!("Due: {due_date}");
    }
    if let Some(minutes) = result.task.estimated_minutes {
        println!("Estimate: {minutes}m");
    }

    Ok(())
}

async fn run_list(args: &[String]) -> anyhow::Result<()> {
    let vault_path = vault_path_from_args(args, false);
    let vault = Vault::connect_or_init(&vault_path).await?;
    vault.initialize_defaults().await?;
    let task_repo = vault.task_repo();
    let tasks = task_repo.list_all().await?;

    if tasks.is_empty() {
        println!("No tasks.");
        return Ok(());
    }

    println!("Tasks:");
    for task in tasks {
        if task.deleted_at.is_some() {
            continue;
        }
        let mut fields = Vec::new();
        if let Some(due_date) = task.due_date {
            fields.push(format!("due {due_date}"));
        }
        if let Some(minutes) = task.estimated_minutes {
            fields.push(format!("{minutes}m"));
        }
        let suffix = if fields.is_empty() {
            String::new()
        } else {
            format!(" ({})", fields.join(", "))
        };
        println!("  {}{}", task.title, suffix);
    }

    Ok(())
}

fn print_help() {
    println!("Mnema desktop stub");
    println!();
    println!("Usage:");
    println!("  mnema-desktop plan [--save] [--vault PATH]");
    println!("  mnema-desktop add \"Task title\" [--due YYYY-MM-DD] [--minutes N] [--vault PATH]");
    println!("  mnema-desktop list [--vault PATH]");
    println!("  mnema-desktop schedule [--date YYYY-MM-DD] [--vault PATH]");
    println!("  mnema-desktop --demo-plan");
    println!();
    println!("Environment:");
    println!("  MNEMA_DATABASE_URL      PostgreSQL connection string");
}

fn run_demo_plan() {
    let (statuses, status_groups, todo_status) = demo_status_catalog();
    let input = SchedulingInput {
        tasks: vec![
            demo_task(
                todo_status.clone(),
                "Write integration notes",
                45,
                Some(time::macros::date!(2026 - 06 - 25)),
            ),
            demo_task(
                todo_status,
                "Review scheduler output",
                60,
                Some(time::macros::date!(2026 - 06 - 25)),
            ),
        ],
        statuses,
        status_groups,
        availability: vec![AvailabilityWindow {
            window: TimeWindow::new(
                datetime!(2026-06-25 09:00 UTC),
                datetime!(2026-06-25 12:00 UTC),
            ),
        }],
        busy_blocks: vec![BusyBlock {
            window: TimeWindow::new(
                datetime!(2026-06-25 10:00 UTC),
                datetime!(2026-06-25 10:30 UTC),
            ),
            source: mnema_app::BusyBlockSource::ExternalCalendar,
            label: Some("Calendar event".into()),
        }],
    };

    let output = GreedyScheduler::default().plan(input);
    print_plan(
        "Demo plan",
        time::macros::date!(2026 - 06 - 25),
        &output.blocks,
    );
}

fn print_saved_schedule(label: &str, date: Date, blocks: &[ScheduleBlock]) {
    println!("{label}: {date}");
    if blocks.is_empty() {
        println!("  No saved schedule blocks.");
        return;
    }

    for block in blocks {
        let title = block
            .title_snapshot
            .as_deref()
            .unwrap_or("(untitled block)");
        println!(
            "  {}-{}  {} [{}]",
            format_hm(block.start_at),
            format_hm(block.end_at),
            title,
            schedule_block_state_label(&block.state)
        );
    }
}

fn schedule_block_state_label(state: &ScheduleBlockState) -> &'static str {
    match state {
        ScheduleBlockState::Proposed => "proposed",
        ScheduleBlockState::Scheduled => "scheduled",
        ScheduleBlockState::Active => "active",
        ScheduleBlockState::Done => "done",
        ScheduleBlockState::Missed => "missed",
        ScheduleBlockState::Cancelled => "cancelled",
    }
}

fn print_plan(label: &str, date: Date, blocks: &[mnema_app::ProposedScheduleBlock]) {
    println!("{label}: {date}");
    if blocks.is_empty() {
        println!("  No scheduled blocks.");
        return;
    }

    for block in blocks {
        println!(
            "  {}-{}  {} ({}m)",
            format_hm(block.window.start),
            format_hm(block.window.end),
            block.title,
            block.required_minutes
        );
    }
}

fn format_hm(value: OffsetDateTime) -> String {
    value
        .format(format_description!("[hour]:[minute]"))
        .unwrap_or_else(|_| value.time().to_string())
}

fn default_workday_availability(date: Date) -> anyhow::Result<Vec<AvailabilityWindow>> {
    let start = date.with_hms(9, 0, 0)?.assume_utc();
    let end = date.with_hms(17, 0, 0)?.assume_utc();
    Ok(vec![AvailabilityWindow {
        window: TimeWindow::new(start, end),
    }])
}

fn vault_path_from_args(args: &[String], allow_positional: bool) -> PathBuf {
    option_arg(args, "--vault")
        .map(PathBuf::from)
        .or_else(|| {
            if !allow_positional {
                return None;
            }
            args.iter()
                .find(|arg| !arg.starts_with("--") && !known_option_value(args, arg))
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from("./vault"))
}

fn first_positional_arg(args: &[String], options_with_values: &[&str]) -> Option<String> {
    let mut value_indexes = HashSet::new();
    for option in options_with_values {
        if let Some(index) = args.iter().position(|arg| arg == option) {
            value_indexes.insert(index + 1);
        }
    }

    args.iter()
        .enumerate()
        .find(|(index, arg)| !arg.starts_with("--") && !value_indexes.contains(index))
        .map(|(_, arg)| arg.clone())
}

fn option_arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}

fn optional_date_arg(args: &[String], name: &str) -> anyhow::Result<Option<Date>> {
    option_arg(args, name)
        .map(|value| Date::parse(&value, format_description!("[year]-[month]-[day]")))
        .transpose()
        .map_err(Into::into)
}

fn optional_u32_arg(args: &[String], name: &str) -> anyhow::Result<Option<u32>> {
    option_arg(args, name)
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(Into::into)
}

fn known_option_value(args: &[String], value: &str) -> bool {
    ["--due", "--minutes", "--vault", "--date"]
        .iter()
        .filter_map(|name| option_arg(args, name))
        .any(|option_value| option_value == value)
}

fn demo_status_catalog() -> (Vec<Status>, Vec<StatusGroup>, StatusId) {
    let todo_group = StatusGroup {
        id: StatusGroupId::new(),
        name: "Not started".into(),
        kind: StatusGroupKind::NotStarted,
    };
    let todo = Status {
        id: StatusId::new(),
        project_id: None,
        name: "To do".into(),
        group_id: todo_group.id.clone(),
        order: 0,
    };

    (vec![todo.clone()], vec![todo_group], todo.id)
}

fn demo_task(
    status_id: StatusId,
    title: &str,
    estimated_minutes: u32,
    due_date: Option<Date>,
) -> Task {
    Task {
        id: TaskId::new(),
        title: title.into(),
        description: None,
        project_id: None,
        list_id: None,
        status_id,
        due_date,
        start_date: None,
        estimated_minutes: Some(estimated_minutes),
        cost_points: None,
        dependencies: Vec::new(),
        milestone_id: None,
        created_at: datetime!(2026-06-25 00:00 UTC),
        updated_at: datetime!(2026-06-25 00:00 UTC),
        deleted_at: None,
    }
}
