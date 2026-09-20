use axum::{
    Router,
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};

use crate::api::AppState;

const INDEX_HTML: &str = include_str!("../web/index.html");
const STYLES_CSS: &str = include_str!("../web/styles.css");
const APP_JS: &str = include_str!("../web/js/app.js");
const API_JS: &str = include_str!("../web/js/api.js");
const STORE_JS: &str = include_str!("../web/js/store.js");
const UI_JS: &str = include_str!("../web/js/ui.js");
const CALENDAR_JS: &str = include_str!("../web/js/features/calendar.js");
const HABITS_JS: &str = include_str!("../web/js/features/habits.js");

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/assets/styles.css", get(styles))
        .route("/assets/js/app.js", get(app_js))
        .route("/assets/js/api.js", get(api_js))
        .route("/assets/js/store.js", get(store_js))
        .route("/assets/js/ui.js", get(ui_js))
        .route("/assets/js/features/calendar.js", get(calendar_js))
        .route("/assets/js/features/habits.js", get(habits_js))
        .fallback(spa_fallback)
}

async fn index() -> Response {
    let mut response = Html(INDEX_HTML).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store, must-revalidate"),
    );
    response
}

async fn styles() -> Response {
    asset(STYLES_CSS, "text/css; charset=utf-8")
}

async fn app_js() -> Response {
    asset(APP_JS, "text/javascript; charset=utf-8")
}

async fn api_js() -> Response {
    asset(API_JS, "text/javascript; charset=utf-8")
}

async fn store_js() -> Response {
    asset(STORE_JS, "text/javascript; charset=utf-8")
}

async fn ui_js() -> Response {
    asset(UI_JS, "text/javascript; charset=utf-8")
}

async fn calendar_js() -> Response {
    asset(CALENDAR_JS, "text/javascript; charset=utf-8")
}

async fn habits_js() -> Response {
    asset(HABITS_JS, "text/javascript; charset=utf-8")
}

async fn spa_fallback() -> Response {
    index().await
}

fn asset(content: &'static str, content_type: &'static str) -> Response {
    let mut response = (StatusCode::OK, content).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store, must-revalidate"),
    );
    response
}
