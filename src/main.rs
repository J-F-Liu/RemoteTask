use axum::{
    Json, Router,
    extract::{Form, State},
    response::Html,
    routing::post,
};
use sea_orm::{Database, DatabaseConnection};
use serde::Deserialize;
use std::env;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::runtime::Runtime;
use tracing::*;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod task;

#[derive(Clone)]
struct AppState {
    conn: DatabaseConnection,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL is not set in .env file");
    let host = env::var("HOST").expect("HOST is not set in .env file");
    let port = env::var("PORT").expect("PORT is not set in .env file");
    let server_url = format!("{host}:{port}");

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=debug", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let conn = Database::connect(db_url)
        .await
        .expect("Database connection failed");

    start_runner(conn.clone());

    let state = AppState { conn };

    // build our application with some routes
    let app = Router::new()
        .route("/run", post(add_task))
        .with_state(state);

    // run it
    let listener = tokio::net::TcpListener::bind(server_url).await.unwrap();
    debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

static RUNNING: AtomicBool = AtomicBool::new(true);
static CHECKING: AtomicBool = AtomicBool::new(true);

fn start_runner(conn: DatabaseConnection) {
    std::thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        while RUNNING.load(Ordering::SeqCst) {
            if CHECKING.load(Ordering::SeqCst) {
                rt.block_on(async {
                    run_tasks(&conn).await.unwrap_or_else(|err| {
                        error!("Failed to run tasks: {}", err);
                    });
                });
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
}

async fn run_tasks(conn: &DatabaseConnection) -> Result<(), sea_orm::DbErr> {
    let tasks = task::pending_tasks(conn).await?;
    if tasks.is_empty() {
        CHECKING.store(false, Ordering::SeqCst);
    }
    for task in tasks {
        info!("Running task: {}", task.id);
        task::update_task(conn, task.id, task::TaskStatus::Running).await?;
        match run_just_task(&task.command) {
            Ok(_) => {
                info!("Task {} completed successfully", task.id);
                task::update_task(conn, task.id, task::TaskStatus::Success).await?;
            }
            Err(err) => {
                error!("Task {} failed: {}", task.id, err);
                task::update_task(conn, task.id, task::TaskStatus::Failed).await?;
            }
        }
    }
    Ok(())
}

#[axum::debug_handler]
async fn add_task(state: State<AppState>, command: String) -> Result<Json<task::Model>, String> {
    let task = task::create_task(&state.conn, command.clone(), command)
        .await
        .map_err(|err| err.to_string())?;
    CHECKING.store(true, Ordering::SeqCst);
    Ok(Json(task))
}

fn run_just_task(args: &str) -> std::io::Result<()> {
    let items = args.split(' ').collect::<Vec<_>>();
    let file = std::fs::File::create("output.log")?;
    let io = Stdio::from(file.try_clone()?);
    let io2 = Stdio::from(file);
    let mut just = Command::new("just")
        .args(items)
        .stdout(io)
        .stderr(io2)
        .spawn()?;
    let status = just.wait()?;
    if status.success() {
        Ok(())
    } else {
        let message = match status.code() {
            Some(code) => format!("Command failed, return code: {code}"),
            None => "Command terminated by signal".to_owned(),
        };
        Err(std::io::Error::new(std::io::ErrorKind::Other, message))
    }
}

async fn show_form() -> Html<&'static str> {
    Html(
        r#"
        <!doctype html>
        <html>
            <head></head>
            <body>
                <form action="/" method="post">
                    <label for="name">
                        Enter your name:
                        <input type="text" name="name">
                    </label>

                    <label>
                        Enter your email:
                        <input type="text" name="email">
                    </label>

                    <input type="submit" value="Subscribe!">
                </form>
            </body>
        </html>
        "#,
    )
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct Input {
    name: String,
    email: String,
}

async fn accept_form(Form(input): Form<Input>) {
    dbg!(&input);
}
