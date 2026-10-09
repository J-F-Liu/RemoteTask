use anyhow::Context;
use axum::{
    Router,
    http::header,
    middleware,
    routing::{get, post},
};
use sea_orm::Database;
use std::env;
use tokio::sync::broadcast;
use tower::ServiceBuilder;
use tower_http::{ServiceBuilderExt, services::ServeDir};
use tracing::*;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod db;
mod project;
mod recipe;
mod service;
mod task;
use service::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    // CLI: generate token and exit
    let mut args = std::env::args();
    if args.next().is_some()
        && let Some(cmd) = args.next()
        && (cmd == "generate-token" || cmd == "--generate-token")
    {
        let user = args.next().unwrap_or_else(|| "user".to_string());
        let days = args
            .next()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(90);
        generate_token(user, days);
        return Ok(());
    }
    let db_url = env::var("DATABASE_URL").unwrap_or("sqlite:./tasks.db?mode=rwc".to_string());
    let host = env::var("HOST").unwrap_or("127.0.0.1".to_string());
    let port = env::var("PORT").unwrap_or("5678".to_string());
    let secret = env::var("APP_SECRET").unwrap_or("".to_string());
    let work_dir = env::var("WORK_DIR")
        .ok()
        .filter(|dir| !dir.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap());
    let output_dir = env::var("OUTPUT_DIR")
        .ok()
        .filter(|dir| !dir.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| work_dir.clone());
    let logs_dir = work_dir.join("logs");
    let server_url = format!("{host}:{port}");

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=debug", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
    info!("Listening on {}", &server_url);
    info!("Work directory: {}", work_dir.display());

    let conn = Database::connect(db_url)
        .await
        .expect("Database connection failed");
    db::migrate(&conn, &work_dir, &output_dir)
        .await
        .expect("Database migration failed");

    let (sender, _) = broadcast::channel(10);
    let (shutdown_tx, _) = broadcast::channel(10);
    let state = AppState {
        conn,
        work_dir,
        output_dir: output_dir.clone(),
        logs_dir: logs_dir.clone(),
        sender: sender.clone(),
        shutdown_tx: shutdown_tx.clone(),
    };

    let runner = start_runner(state.clone());

    // build our application with some routes
    let mut router = Router::new()
        .route("/projects", get(list_projects).post(add_project))
        .route("/project/{id}", get(get_project))
        .route("/recipes", post(add_recipe))
        .route("/run", post(add_task))
        .route("/cancel/{id}", post(cancel_task))
        .route("/reset/{id}", post(reset_task))
        .route("/list/{page}", get(list_task))
        .route("/status", get(task_status_sse))
        .with_state(state);
    if !secret.is_empty() {
        router = router.route_layer(middleware::from_fn_with_state(secret, validate_jwt))
    }
    router = router
        .nest_service(
            "/logs",
            ServiceBuilder::new()
                .override_response_header(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("text/plain; charset=utf-8"),
                )
                .service(ServeDir::new(logs_dir)),
        )
        .nest_service("/package", ServeDir::new(&output_dir))
        .fallback_service(ServeDir::new("public").precompressed_br());

    // run it
    let listener = tokio::net::TcpListener::bind(server_url)
        .await
        .context("failed to bind TCP listener")?;
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal(sender, shutdown_tx, runner))
        .await
        .context("axum::serve failed")?;
    Ok(())
}
