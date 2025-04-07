use axum::{
    Json, Router,
    extract::{Form, Path, State},
    http::StatusCode,
    response::Html,
    routing::{get, post},
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
    let db_url = env::var("DATABASE_URL").unwrap_or("sqlite:./tasks.db?mode=rwc".to_string());
    let host = env::var("HOST").unwrap_or("127.0.0.1".to_string());
    let port = env::var("PORT").unwrap_or("5678".to_string());
    let work_dir = env::var("WORK_DIR")
        .map(|dir| std::path::PathBuf::from(dir))
        .unwrap_or(env::current_dir().unwrap());
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

    start_runner(conn.clone(), work_dir);

    let state = AppState { conn };

    // build our application with some routes
    let app = Router::new()
        .route("/run", post(add_task))
        .route("/list/{page}", get(list_task))
        .with_state(state);

    // run it
    let listener = tokio::net::TcpListener::bind(server_url).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

static RUNNING: AtomicBool = AtomicBool::new(true);
static CHECKING: AtomicBool = AtomicBool::new(true);

fn start_runner(conn: DatabaseConnection, work_dir: std::path::PathBuf) {
    std::thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        while RUNNING.load(Ordering::SeqCst) {
            if CHECKING.load(Ordering::SeqCst) {
                rt.block_on(async {
                    run_tasks(&conn, &work_dir).await.unwrap_or_else(|err| {
                        error!("Failed to run tasks: {}", err);
                    });
                });
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
}

async fn run_tasks(
    conn: &DatabaseConnection,
    work_dir: &std::path::Path,
) -> Result<(), sea_orm::DbErr> {
    let tasks = task::pending_tasks(conn).await?;
    if tasks.is_empty() {
        CHECKING.store(false, Ordering::SeqCst);
    }
    for task in tasks {
        info!("Running task: {}", task.id);
        task::update_task(conn, task.id, task::TaskStatus::Running).await?;
        let log_dir = work_dir.join("logs").join(task.month());
        if !log_dir.is_dir() {
            std::fs::create_dir_all(&log_dir)
                .unwrap_or_else(|err| error!("Failed to create log directory: {}", err));
        }
        let log_file = log_dir.join(format!("{}.log", task.id));
        let output_file = task.output.map(|path| work_dir.join(path));
        match run_just_task(&task.command, work_dir, &log_file, output_file.as_ref()) {
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

async fn add_task(
    state: State<AppState>,
    command: String,
) -> Result<Json<task::Model>, (StatusCode, String)> {
    let task = task::create_task(&state.conn, command.clone(), command)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    CHECKING.store(true, Ordering::SeqCst);
    Ok(Json(task))
}

#[axum::debug_handler]
async fn list_task(
    state: State<AppState>,
    Path(page): Path<u64>,
) -> Result<Json<(Vec<task::Model>, u64)>, (StatusCode, String)> {
    if page == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Page number must be greater than 0".to_string(),
        ));
    }
    let (tasks, pages) = task::recent_tasks(&state.conn, 10, page - 1)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json((tasks, pages)))
}

fn run_just_task(
    command: &str,
    work_dir: &std::path::Path,
    log_file: &std::path::Path,
    output_file: Option<&std::path::PathBuf>,
) -> std::io::Result<()> {
    let items = command.split(' ').collect::<Vec<_>>();
    let file = std::fs::File::create(log_file)?;
    let io = Stdio::from(file.try_clone()?);
    let io2 = Stdio::from(file);
    let mut just = Command::new("just")
        .current_dir(work_dir)
        .args(items)
        .stdout(io)
        .stderr(io2)
        .spawn()?;
    let status = just.wait()?;
    if status.success() {
        if let Some(output_file) = output_file {
            if output_file.is_file() {
                Ok(())
            } else {
                let message = format!(
                    "Command finished, but output file {} does not exist",
                    output_file.display()
                );
                Err(std::io::Error::new(std::io::ErrorKind::Other, message))
            }
        } else {
            Ok(())
        }
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
