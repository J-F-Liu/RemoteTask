use crate::task;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use sea_orm::DatabaseConnection;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use tokio::runtime::Runtime;
use tracing::*;

static RUNNING: AtomicBool = AtomicBool::new(true);
static CHECKING: AtomicBool = AtomicBool::new(true);

#[derive(Clone)]
pub struct AppState {
    pub conn: DatabaseConnection,
    pub work_dir: std::path::PathBuf,
}

pub fn start_runner(state: AppState, output_dir: std::path::PathBuf) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        while RUNNING.load(Ordering::SeqCst) {
            if CHECKING.load(Ordering::SeqCst) {
                rt.block_on(async {
                    run_tasks(&state.conn, &state.work_dir, &output_dir)
                        .await
                        .unwrap_or_else(|err| {
                            error!("Failed to run tasks: {}", err);
                        });
                });
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    })
}

pub async fn shutdown_signal(runner: JoinHandle<()>) {
    // 监听 Ctrl+C 信号
    tokio::signal::ctrl_c().await.expect("Listen for Ctrl+C");

    info!("Shutdown server...");
    RUNNING.store(false, Ordering::SeqCst);
    runner.join().expect("Terminate runner");
}

pub async fn run_tasks(
    conn: &DatabaseConnection,
    work_dir: &std::path::Path,
    output_dir: &std::path::Path,
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
        let output_file = task.output.map(|path| output_dir.join(path));
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
    let task = task::create_task(&state.conn, name, command, output)
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
    let task = task::update_task(&state.conn, id, task::TaskStatus::Pending)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    CHECKING.store(true, Ordering::SeqCst);
    Ok(Json(task))
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
    let output = Command::new("just")
        .current_dir(&state.work_dir)
        .arg("--list")
        .output()
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

pub fn run_just_task(
    command: &str,
    work_dir: &std::path::Path,
    log_file: &std::path::Path,
    output_file: Option<&std::path::PathBuf>,
) -> std::io::Result<()> {
    let items = command.split(' ').collect::<Vec<_>>();
    let mut file = std::fs::File::create(log_file)?;
    let io = Stdio::from(file.try_clone()?);
    let io2 = Stdio::from(file.try_clone()?);
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
}
