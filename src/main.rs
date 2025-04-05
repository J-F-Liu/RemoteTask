use axum::{
    Router,
    extract::{Form, State},
    response::Html,
    routing::post,
};
use sea_orm::{Database, DatabaseConnection};
use serde::Deserialize;
use std::env;
use std::process::{Command, Stdio};
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

    let state = AppState { conn };

    // build our application with some routes
    let app = Router::new()
        .route("/run", post(run_task))
        .with_state(state);

    // run it
    let listener = tokio::net::TcpListener::bind(server_url).await.unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}
async fn run_task(state: State<AppState>, command: String) -> Result<Vec<u8>, String> {
    let task = task::create_task(&state.conn, command)
        .await
        .map_err(|err| err.to_string())?;
    run_io_task(&task.command.unwrap()).map_err(|err| err.to_string())
}

fn run_io_task(args: &str) -> std::io::Result<Vec<u8>> {
    let items = args.split(' ').collect::<Vec<_>>();

    let file = std::fs::File::create("output.log")?;
    let io = Stdio::from(file.try_clone()?);
    let io2 = Stdio::from(file);
    let output = Command::new("just")
        .args(items)
        .stdout(io)
        .stderr(io2)
        .output()?;
    Ok(output.stdout)
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
