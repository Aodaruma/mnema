use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use mnema_app::default_scheduling_preferences;
use mnema_core::prelude::*;
use mnema_infra::calendar::MemoryCredentialStore;
use mnema_infra::db::Vault;
use mnema_server::{api::AppState, config::ServerConfig, router};
use serde_json::{Value, json};
use tempfile::tempdir;
use tower::ServiceExt;

async fn test_app() -> (tempfile::TempDir, Router) {
    let temp = tempdir().unwrap();
    let vault_path = temp.path().join("vault");
    let database_path = temp.path().join("mnema.sqlite");
    let vault = Vault::connect_or_init_with_sqlite_path(&vault_path, &database_path)
        .await
        .unwrap();
    vault.initialize_defaults().await.unwrap();
    vault
        .scheduling_preferences_repo()
        .upsert(default_scheduling_preferences(
            local_user_id(),
            "Etc/UTC",
            time::OffsetDateTime::now_utc(),
        ))
        .await
        .unwrap();
    let config = ServerConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        vault_path,
        timezone: "Etc/UTC".to_string(),
        timezone_offset: "+00:00".to_string(),
        planning_start: "09:00".to_string(),
        planning_end: "17:00".to_string(),
        refresh_seconds: 30,
        automation_mode: mnema_server::config::AutomationMode::Off,
        automation_interval_seconds: 0,
    };
    (
        temp,
        router(AppState::with_credentials(
            vault,
            Arc::new(config),
            Arc::new(MemoryCredentialStore::default()),
        )),
    )
}

async fn call(app: &Router, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    let payload = if let Some(body) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(serde_json::to_vec(&body).unwrap())
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(builder.body(payload).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

#[tokio::test]
async fn task_plan_and_schedule_round_trip() {
    let (_temp, app) = test_app().await;

    let (status, health) = call(&app, Method::GET, "/api/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["status"], "ok");
    assert_eq!(health["backend"], "sqlite");

    let (status, created) = call(
        &app,
        Method::POST,
        "/api/tasks",
        Some(json!({
            "title": "Server smoke task",
            "description": "Created through the REST boundary",
            "due_date": "2099-01-15",
            "estimated_minutes": 45
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let task_id = created["id"].as_str().unwrap();

    let plan_input = json!({
        "date": "2099-01-15",
        "availability_start": "09:00",
        "availability_end": "17:00",
        "timezone_offset": "+00:00"
    });
    let (status, preview) = call(
        &app,
        Method::POST,
        "/api/plans/today/preview",
        Some(plan_input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["applied"], false);
    assert_eq!(preview["blocks"].as_array().unwrap().len(), 1);

    let (status, applied) = call(
        &app,
        Method::POST,
        "/api/plans/today/apply",
        Some(plan_input),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(applied["applied"], true);

    let (status, schedule) = call(&app, Method::GET, "/api/schedule?date=2099-01-15", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(schedule["blocks"].as_array().unwrap().len(), 1);

    let (status, updated) = call(
        &app,
        Method::PUT,
        &format!("/api/tasks/{task_id}"),
        Some(json!({
            "title": "Updated server task",
            "description": null,
            "due_date": "2099-01-15",
            "estimated_minutes": 30,
            "completed": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["completed"], true);

    let (status, _) = call(&app, Method::DELETE, &format!("/api/tasks/{task_id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn embedded_web_shell_is_served() {
    let (_temp, app) = test_app().await;
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "no-cache, no-store, must-revalidate"
    );
    let html = response.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&html);
    assert!(html.contains("Mnema"));
    assert!(html.contains(">Calendar<"));
    assert!(html.contains(">Habits<"));
    assert!(html.contains("class=\"skip-link\""));
    assert!(html.contains("aria-labelledby=\"task-dialog-title\""));
    assert!(html.contains("aria-labelledby=\"habit-dialog-title\""));
    assert!(html.contains("<form id=\"task-form\">"));
    assert!(html.contains("<form id=\"habit-form\">"));
    assert!(!html.contains("method=\"dialog\""));
    assert!(
        html.contains(
            "name=\"estimated_minutes\" type=\"number\" min=\"1\" max=\"10080\" step=\"1\""
        )
    );
    assert!(html.contains("/assets/styles.css?v=0.0.1-s3"));
    assert!(html.contains("/assets/js/app.js?v=0.0.1-s3"));

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/assets/js/features/calendar.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "no-cache, no-store, must-revalidate"
    );
    let javascript = response.into_body().collect().await.unwrap().to_bytes();
    let javascript = String::from_utf8_lossy(&javascript);
    assert!(javascript.contains("startOAuth"));
    assert!(javascript.contains("writeback"));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/assets/js/ui.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let javascript = response.into_body().collect().await.unwrap().to_bytes();
    let javascript = String::from_utf8_lossy(&javascript);
    assert!(javascript.contains("aria-current"));
    assert!(javascript.contains("retry-refresh"));
    assert!(javascript.contains("change.kind !== \"UNCHANGED\""));
}

#[tokio::test]
async fn preferences_habits_and_fingerprinted_auto_schedule_round_trip() {
    let (_temp, app) = test_app().await;

    let (status, mut preferences) =
        call(&app, Method::GET, "/api/scheduling/preferences", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preferences["timezone"], "Etc/UTC");
    preferences["default_travel_buffer_minutes"] = json!(25);
    let (status, updated_preferences) = call(
        &app,
        Method::PUT,
        "/api/scheduling/preferences",
        Some(preferences),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_preferences["default_travel_buffer_minutes"], 25);

    let (status, habit) = call(
        &app,
        Method::POST,
        "/api/habits",
        Some(json!({
            "title": "Daily walk",
            "schedule": { "kind": "DAILY", "weekdays": [] },
            "duration_minutes": 30,
            "preferred_window": null,
            "flexibility": "FLEXIBLE"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let habit_id = habit["id"].as_str().unwrap();

    let (status, expanded) = call(
        &app,
        Method::POST,
        "/api/habits/occurrences/expand",
        Some(json!({
            "start_date": "2099-01-12",
            "end_date_exclusive": "2099-01-19"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let occurrences = expanded["occurrences"].as_array().unwrap();
    assert_eq!(occurrences.len(), 7);
    let skip_id = occurrences[0]["id"].as_str().unwrap();
    let snooze_id = occurrences[1]["id"].as_str().unwrap();

    let (status, skipped) = call(
        &app,
        Method::POST,
        &format!("/api/habits/occurrences/{skip_id}/skip"),
        Some(json!({ "reason": "travel" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(skipped["state"], "SKIPPED");
    let (status, snoozed) = call(
        &app,
        Method::POST,
        &format!("/api/habits/occurrences/{snooze_id}/snooze"),
        Some(json!({ "until": "2099-01-13T12:00:00Z" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(snoozed["state"], "SNOOZED");

    let (status, _) = call(
        &app,
        Method::POST,
        "/api/tasks",
        Some(json!({
            "title": "Seven day task",
            "due_date": "2099-01-12",
            "estimated_minutes": 45
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let auto_input = json!({
        "start_date": "2099-01-12",
        "days": 7,
        "timezone": "Etc/UTC",
        "named_hours": []
    });
    let (status, preview) = call(
        &app,
        Method::POST,
        "/api/auto-schedule/preview",
        Some(auto_input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let fingerprint = preview["fingerprint"].as_str().unwrap();
    assert_eq!(fingerprint.len(), 64);
    assert!(preview["blocks"].as_array().unwrap().len() >= 2);

    let mut apply_input = auto_input.clone();
    apply_input["fingerprint"] = json!("");
    let (status, _) = call(
        &app,
        Method::POST,
        "/api/auto-schedule/apply",
        Some(apply_input),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let mut apply_input = auto_input;
    apply_input["fingerprint"] = json!(fingerprint);
    let (status, applied) = call(
        &app,
        Method::POST,
        "/api/auto-schedule/apply",
        Some(apply_input),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!applied["blocks"].as_array().unwrap().is_empty());

    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/api/habits/{habit_id}/disable"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, habits) = call(&app, Method::GET, "/api/habits", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(habits["habits"].as_array().unwrap().is_empty());

    let (status, accounts) = call(&app, Method::GET, "/api/calendar/accounts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(accounts["accounts"].as_array().unwrap().is_empty());
}
