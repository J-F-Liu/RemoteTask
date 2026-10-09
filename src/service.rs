use crate::{project, recipe, task};
use axum::{
    Json,
    extract::{Path, Query, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{
        IntoResponse,
        sse::{Event, Sse},
    },
};
use axum_extra::extract::CookieJar;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_stream::StreamExt as TokioStreamExt;
use tokio_stream::wrappers::{BroadcastStream, errors::BroadcastStreamRecvError};
use tracing::{error, info};

pub type ApiError = (StatusCode, String);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskStatusEvent {
    pub task_id: i32,
    pub status: String,
    pub timestamp: String,
}

#[derive(Clone, Debug)]
pub struct ShutdownSignal;

static RUNNING: AtomicBool = AtomicBool::new(true);

#[derive(Clone)]
pub struct AppState {
    pub conn: DatabaseConnection,
    pub work_dir: PathBuf,
    pub output_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub sender: broadcast::Sender<TaskStatusEvent>,
    pub shutdown_tx: broadcast::Sender<ShutdownSignal>,
}

fn internal_error(err: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

/// Trim the value and reject the request when it is empty.
fn required(value: &str, field: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if value.is_empty() {
        Err((StatusCode::BAD_REQUEST, format!("{field} is required")))
    } else {
        Ok(value.to_string())
    }
}

async fn find_project(state: &AppState, id: i32) -> Result<project::Model, ApiError> {
    project::find(&state.conn, id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Project {id} not found")))
}

pub fn start_runner(state: AppState) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            if !RUNNING.load(Ordering::SeqCst) {
                break;
            }
            if let Err(err) = run_tasks(&state).await {
                error!("Failed to run tasks: {}", err);
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

pub async fn run_tasks(state: &AppState) -> Result<(), sea_orm::DbErr> {
    let tasks = task::pending_tasks(&state.conn).await?;
    for task in tasks {
        info!("Running task: {}", task.id);
        update_task(state, task.id, task::TaskStatus::Running).await?;
        let log_dir = state.logs_dir.join(task.month());
        if !log_dir.is_dir() {
            std::fs::create_dir_all(&log_dir)
                .unwrap_or_else(|err| error!("Failed to create log directory: {}", err));
        }
        let log_file = log_dir.join(format!("{}.log", task.id));
        let (work_dir, output_dir) = match project::find(&state.conn, task.project_id).await? {
            Some(project) => (PathBuf::from(project.path), PathBuf::from(project.output)),
            None => (state.work_dir.clone(), state.output_dir.clone()),
        };
        let output_file = task.output.map(|path| output_dir.join(path));
        match run_just_task(&task.command, &work_dir, &log_file, output_file.as_ref()).await {
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

#[derive(Deserialize)]
pub struct RunRequest {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub project: Option<i32>,
}

pub async fn add_task(
    state: State<AppState>,
    Json(payload): Json<RunRequest>,
) -> Result<Json<task::Model>, ApiError> {
    let (project_id, dir) = match payload.project {
        Some(id) => {
            let project = find_project(&state, id).await?;
            (project.id, project.path)
        }
        None => (0, state.work_dir.to_string_lossy().into_owned()),
    };
    let task = task::create_task(
        &state.conn,
        dir,
        payload.name,
        payload.command,
        payload.output,
        project_id,
    )
    .await
    .map_err(internal_error)?;
    Ok(Json(task))
}

pub async fn cancel_task(state: State<AppState>, Path(id): Path<i32>) -> Result<String, ApiError> {
    task::delete_task(&state.conn, id)
        .await
        .map(|value| value.to_string())
        .map_err(internal_error)
}

pub async fn reset_task(
    state: State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<task::Model>, ApiError> {
    let task = update_task(&state, id, task::TaskStatus::Pending)
        .await
        .map_err(internal_error)?;
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

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub project: Option<i32>,
}

pub async fn list_task(
    state: State<AppState>,
    Path(page): Path<u64>,
    Query(query): Query<ListQuery>,
) -> Result<Json<(Vec<task::Model>, u64)>, ApiError> {
    if page == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Page number must be greater than 0".to_string(),
        ));
    }
    let (tasks, pages) = task::recent_tasks(&state.conn, 10, page - 1, query.project)
        .await
        .map_err(internal_error)?;
    Ok(Json((tasks, pages)))
}

/// Get a project together with its recipes.
pub async fn get_project(
    state: State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<(project::Model, Vec<recipe::Model>)>, ApiError> {
    let project = find_project(&state, id).await?;
    let recipes = recipe::list(&state.conn, project.id)
        .await
        .map_err(internal_error)?;
    Ok(Json((project, recipes)))
}

pub async fn list_projects(state: State<AppState>) -> Result<Json<Vec<project::Model>>, ApiError> {
    project::list(&state.conn)
        .await
        .map(Json)
        .map_err(internal_error)
}

#[derive(Deserialize)]
pub struct ProjectRequest {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub output: String,
}

pub async fn add_project(
    state: State<AppState>,
    Json(payload): Json<ProjectRequest>,
) -> Result<Json<project::Model>, ApiError> {
    let name = required(&payload.name, "name")?;
    let path = required(&payload.path, "path")?;
    if !std::path::Path::new(&path).is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{path} is not a directory"),
        ));
    }
    if project::find_by_name(&state.conn, &name)
        .await
        .map_err(internal_error)?
        .is_some()
    {
        return Err((
            StatusCode::CONFLICT,
            format!("Project {name} already exists"),
        ));
    }
    let output = match payload.output.trim() {
        "" => path.clone(),
        output => output.to_string(),
    };
    project::create(&state.conn, name, path, output)
        .await
        .map(Json)
        .map_err(internal_error)
}

#[derive(Deserialize)]
pub struct RecipeRequest {
    pub project: i32,
    pub name: String,
    pub command: String,
}

pub async fn add_recipe(
    state: State<AppState>,
    Json(payload): Json<RecipeRequest>,
) -> Result<Json<recipe::Model>, ApiError> {
    let name = required(&payload.name, "name")?;
    let command = required(&payload.command, "command")?;
    find_project(&state, payload.project).await?;
    recipe::upsert(&state.conn, payload.project, name, command)
        .await
        .map(Json)
        .map_err(internal_error)
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
                    Err(std::io::Error::other(message))
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
            Err(std::io::Error::other(message))
        }
    })
    .await
    .map_err(|_| std::io::Error::other("spawn_blocking failed"))?
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
