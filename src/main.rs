use anyhow::Context;
use axum::{
    Router,
    routing::{get, post},
};
use sea_orm::Database;
use std::env;
use tower_http::services::ServeDir;
use tracing::*;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod service;
mod task;
use service::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let db_url = env::var("DATABASE_URL").unwrap_or("sqlite:./tasks.db?mode=rwc".to_string());
    let host = env::var("HOST").unwrap_or("127.0.0.1".to_string());
    let port = env::var("PORT").unwrap_or("5678".to_string());
    let work_dir = env::var("WORK_DIR")
        .map(|dir| std::path::PathBuf::from(dir))
        .unwrap_or(env::current_dir().unwrap());
    let output_dir = env::var("OUTPUT_DIR")
        .map(|dir| std::path::PathBuf::from(dir))
        .unwrap_or(work_dir.clone());
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
    task::create_table_if_not_exists(&conn)
        .await
        .expect("Failed to create table");

    let runner = start_runner(conn.clone(), work_dir, output_dir.clone());

    let state = AppState { conn };

    // build our application with some routes
    let router = Router::new()
        .route("/run", post(add_task))
        .route("/cancel/{id}", post(cancel_task))
        .route("/reset/{id}", post(reset_task))
        .route("/list/{page}", get(list_task))
        .with_state(state)
        .nest_service("/logs", ServeDir::new(logs_dir))
        .nest_service("/package", ServeDir::new(output_dir))
        .fallback_service(ServeDir::new("public").precompressed_br());

    // run it
    let listener = tokio::net::TcpListener::bind(server_url)
        .await
        .context("failed to bind TCP listener")?;
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal(runner))
        .await
        .context("axum::serve failed")?;
    Ok(())
}
