use crate::task;
use axum::{
    Json,
    extract::{Path, Request, State, Query},
    http::StatusCode,
    middleware::Next,
    response::{
        IntoResponse, Redirect, sse::{Event, Sse}
    },
};
use axum_extra::extract::CookieJar;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_stream::StreamExt as TokioStreamExt;
use tracing::{error, info};
use tokio_stream::wrappers::{BroadcastStream, errors::BroadcastStreamRecvError};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskStatusEvent {
    pub task_id: i32,
    pub status: String,
    pub timestamp: String,
}

#[derive(Clone, Debug)]
pub struct ShutdownSignal;

static RUNNING: AtomicBool = AtomicBool::new(true);
static CHECKING: AtomicBool = AtomicBool::new(true);

#[derive(Clone)]
pub struct AppState {
    pub conn: DatabaseConnection,
    pub work_dir: Arc<RwLock<PathBuf>>,
    pub work_dirs: Vec<PathBuf>,
    pub logs_dir: PathBuf,
    pub sender: broadcast::Sender<TaskStatusEvent>,
    pub shutdown_tx: broadcast::Sender<ShutdownSignal>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DirInfo {
    pub current: String,
    pub all_dirs: Vec<String>,
}

pub fn start_runner(state: AppState, output_dir: std::path::PathBuf) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            if !RUNNING.load(Ordering::SeqCst) {
                break;
            }
            if CHECKING.load(Ordering::SeqCst) {
                if let Err(err) = run_tasks(&state, &output_dir).await {
                    error!("Failed to run tasks: {}", err);
                }
            }
            interval.tick().await;
        }
    })
}

pub async fn shutdown_signal(
    sender: broadcast::Sender<TaskStatusEvent>,
    shutdown_tx: broadcast::Sender<ShutdownSignal>,
    runner: JoinHandle<()>,
) {
    // Wait for Ctrl+C signal
    tokio::signal::ctrl_c().await.expect("Listen for Ctrl+C");

    info!("Shutdown server...");
    RUNNING.store(false, Ordering::SeqCst);

    // Wait for runner to finish
    match runner.await {
        Ok(_) => info!("Runner finished"),
        Err(err) => error!("Runner join error: {}", err),
    }

    // Send shutdown signal to all SSE clients
    let _ = shutdown_tx.send(ShutdownSignal);
    info!("Shutdown signal sent to SSE clients");

    // Drop sender to close task status channel
    drop(sender);

    // Give SSE clients a moment to close
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    info!("SSE connections closed");
}

pub async fn run_tasks(
    state: &AppState,
    output_dir: &std::path::Path,
) -> Result<(), sea_orm::DbErr> {
    let tasks = task::pending_tasks(&state.conn).await?;
    if tasks.is_empty() {
        CHECKING.store(false, Ordering::SeqCst);
    }
    for task in tasks {
        info!("Running task: {}", task.id);
        update_task(state, task.id, task::TaskStatus::Running).await?;
        let log_dir = state.logs_dir.join(task.month());
        if !log_dir.is_dir() {
            std::fs::create_dir_all(&log_dir)
                .unwrap_or_else(|err| error!("Failed to create log directory: {}", err));
        }
        let log_file = log_dir.join(format!("{}.log", task.id));
        let output_file = task.output.map(|path| output_dir.join(path));
        let work_dir = state.work_dir.read().unwrap().clone();
        match run_just_task(
            &task.command,
            &work_dir,
            &log_file,
            output_file.as_ref(),
        )
        .await
        {
            Ok(_) => {
                info!("Task {} completed successfully", task.id);
                update_task(state, task.id, task::TaskStatus::Success).await?;
            }
            Err(err) => {
                error!("Task {} failed: {}", task.id, err);
                update_task(state, task.id, task::TaskStatus::Failed).await?;
            }
        }
    }
    Ok(())
}

pub async fn add_task(
    state: State<AppState>,
    Json(mut payload): Json<HashMap<String, String>>,
) -> Result<Json<task::Model>, (StatusCode, String)> {
    let name = payload
        .remove("name")
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "name is required".to_string()))?;
    let command = payload
        .remove("command")
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "command is required".to_string()))?;
    let output = payload.remove("output");
    let work_dir = state.work_dir.read().unwrap().to_str().unwrap().to_string().clone();
    let task = task::create_task(&state.conn, work_dir, name, command, output)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    CHECKING.store(true, Ordering::SeqCst);
    Ok(Json(task))
}

pub async fn cancel_task(
    state: State<AppState>,
    Path(id): Path<i32>,
) -> Result<String, (StatusCode, String)> {
    task::delete_task(&state.conn, id)
        .await
        .map(|value| value.to_string())
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

pub async fn reset_task(
    state: State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<task::Model>, (StatusCode, String)> {
    let task = update_task(&state, id, task::TaskStatus::Pending)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    CHECKING.store(true, Ordering::SeqCst);
    Ok(Json(task))
}

// When updating task status, notify via channel
pub async fn update_task(
    state: &AppState,
    id: i32,
    status: task::TaskStatus,
) -> Result<task::Model, sea_orm::DbErr> {
    let task = task::update_task(&state.conn, id, status).await?;
    let event = TaskStatusEvent {
        task_id: id,
        status: format!("{:?}", status),
        timestamp: chrono::Local::now().to_rfc3339(),
    };
    let _ = state.sender.send(event);
    Ok(task)
}

// SSE endpoint for task status updates
pub async fn task_status_sse(
    State(state): State<AppState>,
) -> Sse<impl futures::Stream<Item = Result<Event, BroadcastStreamRecvError>>> {
    use futures::stream::select;

    let receiver = state.sender.subscribe();
    let shutdown_rx = state.shutdown_tx.subscribe();

    let stream = TokioStreamExt::map(BroadcastStream::new(receiver), |event| match event {
        Ok(status_event) => {
            let data = json!({
                "task_id": status_event.task_id,
                "status": status_event.status,
                "timestamp": status_event.timestamp,
            })
            .to_string();
            Ok(Event::default()
                .id(status_event.task_id.to_string())
                .event("task_status")
                .data(data))
        }
        Err(e) => Err(e),
    });

    // Create a stream that terminates on shutdown signal
    let shutdown_stream = TokioStreamExt::map(
        BroadcastStream::new(shutdown_rx),
        |_| -> Result<Event, BroadcastStreamRecvError> {
            // Shutdown signal received, return error to terminate stream
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(0))
        },
    );

    // Merge both streams - first one to emit wins
    let combined = select(stream, shutdown_stream);

    Sse::new(combined)
}

pub async fn list_task(
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

pub async fn get_available(
    state: State<AppState>,
) -> Result<Json<Vec<String>>, (StatusCode, String)> {
    let work_dir = state.work_dir.clone();
    let output = tokio::task::spawn_blocking(move || {
        let work_dir = work_dir.read().unwrap();
        Command::new("just")
            .current_dir(work_dir.as_path())
            .arg("--list")
            .output()
    })
    .await
    .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
    .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    if output.status.success() {
        Ok(Json(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .skip(1) // skip "Available recipes:"
                .map(|line| line.trim().to_string())
                .collect::<Vec<_>>(),
        ))
    } else {
        Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

pub async fn get_dir(
    state: State<AppState>,
) -> Json<DirInfo> {
    let current = state.work_dir.read().unwrap().to_str().unwrap().to_string();
    let all_dirs = state.work_dirs.iter()
        .map(|d| d.to_str().unwrap().to_string())
        .collect::<Vec<_>>();
    Json(DirInfo { current, all_dirs })
}

#[derive(Deserialize)]
pub struct ChangeDirParam {
    dir: String
}

pub async fn change_dir(
    state: State<AppState>,
    Query(param): Query<ChangeDirParam>
) -> Redirect {
    let dir = PathBuf::from(param.dir);
    if state.work_dirs.contains(&dir) {
        let mut w = state.work_dir.write().unwrap();
        *w = dir;
    }
    Redirect::to("/")
}

pub async fn run_just_task(
    command: &str,
    work_dir: &std::path::Path,
    log_file: &std::path::Path,
    output_file: Option<&std::path::PathBuf>,
) -> std::io::Result<()> {
    let cmd = command.to_string();
    let wd = work_dir.to_path_buf();
    let log = log_file.to_path_buf();
    let out = output_file.cloned();

    tokio::task::spawn_blocking(move || {
        let items = cmd.split(' ').collect::<Vec<_>>();
        let mut file = std::fs::File::create(&log)?;
        let io = Stdio::from(file.try_clone()?);
        let io2 = Stdio::from(file.try_clone()?);
        let mut just = Command::new("just")
            .current_dir(&wd)
            .args(items)
            .stdout(io)
            .stderr(io2)
            .spawn()?;
        let status = just.wait()?;

        if status.success() {
            if let Some(output_file) = out {
                if output_file.is_file() {
                    Ok(())
                } else {
                    let message = format!(
                        "Command finished, but output file {} does not exist",
                        output_file.display()
                    );
                    file.write_all(message.as_bytes())?;
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
            file.write_all(message.as_bytes())?;
            Err(std::io::Error::new(std::io::ErrorKind::Other, message))
        }
    })
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "spawn_blocking failed"))?
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct JwtPayload {
    pub user: String,
    pub role: String,
    pub iat: i64,
    pub exp: i64,
}

pub async fn validate_jwt(
    secret: State<String>,
    request: Request,
    next: Next,
) -> impl IntoResponse {
    let jar = CookieJar::from_headers(request.headers());
    if let Some(token) = jar.get("token").map(|c| c.value()) {
        match jsonwebtoken::decode::<JwtPayload>(
            token,
            &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
            &jsonwebtoken::Validation::default(),
        ) {
            Ok(_payload) => {
                // info!("JWT payload: {:?}", payload.claims);
                return next.run(request).await;
            }
            Err(err) => {
                error!("JWT validation failed: {}", err);
            }
        }
    }
    (StatusCode::UNAUTHORIZED, "Invalid token".to_string()).into_response()
}

pub fn generate_token(user: String, days: i64) {
    dotenvy::dotenv().ok();
    let secret = match std::env::var("APP_SECRET") {
        Ok(s) if !s.is_empty() => s,
        _ => {
            eprintln!("APP_SECRET not set");
            std::process::exit(1);
        }
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let payload = JwtPayload {
        user,
        role: "admin".to_string(),
        iat: now,
        exp: now + 60 * 60 * 24 * days,
    };
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &payload,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();
    // write token to token.txt and also print it
    if let Err(err) = std::fs::write("token.txt", format!("{}\n", token)) {
        eprintln!("Failed to write token.txt: {}", err);
    }
    println!("Token generated and saved to token.txt.");
}
