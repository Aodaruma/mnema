use mnema_app::{
    AvailabilityWindow, BusyBlock, GreedyScheduler, PlanTodayRequest, PlanTodayService,
    SchedulingInput, TimeWindow,
};
use mnema_core::prelude::*;
use mnema_infra::db::Vault;
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

    let vault_path = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./vault"));

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
    if !result.output.unscheduled.is_empty() {
        println!("Unscheduled tasks: {}", result.output.unscheduled.len());
    }
    if result.output.blocks.is_empty() && result.output.unscheduled.is_empty() {
        println!("No candidate tasks for today.");
    }

    Ok(())
}

fn print_help() {
    println!("Mnema desktop stub");
    println!();
    println!("Usage:");
    println!("  mnema-desktop [vault_path]");
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
