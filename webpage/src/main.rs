use dioxus::prelude::*;
use serde::de::DeserializeOwned;
use serde_json::json;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::window;

const PICO_CSS: Asset = asset!("/assets/pico.min.css");
const MAIN_CSS: Asset = asset!("/assets/main.css");

mod project;
mod recipe;
mod task;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        document::Link { rel: "stylesheet", href: PICO_CSS }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        Head {}
        Main {}
    }
}

#[component]
fn Head() -> Element {
    rsx! {
        header { class: "container",
            h1 { "Remote Task Runner" }
            hr {}
        }
    }
}

#[component]
fn Main() -> Element {
    let page = use_signal(|| 1);
    let current = use_signal(|| query_project().unwrap_or(0));
    let new_project = use_signal(|| false);
    let new_recipe = use_signal(|| false);
    let task_updates = use_signal(HashMap::<i32, String>::new);

    let projects =
        use_resource(move || async move { get_json::<Vec<project::Project>>("/projects").await });

    // Fall back to the first project when the URL points to an unknown one
    use_effect(move || {
        let Some(list) = projects() else { return };
        if !list.iter().any(|project| project.id == current()) {
            if let Some(id) = list.first().map(|project| project.id) {
                select_project(current, id);
            }
        }
    });

    let resource = use_resource(move || async move {
        let project = current();
        if project == 0 {
            return Ok((Vec::<task::Task>::new(), 0));
        }
        reqwest::Client::new()
            .get(format!("{}/list/{}?project={}", origin(), page(), project))
            .send()
            .await
            .unwrap()
            .json::<(Vec<task::Task>, i32)>()
            .await
    });

    // Set up SSE connection when component mounts
    use_effect(move || {
        spawn(async move {
            connect_sse(task_updates).await;
        });
    });

    rsx! {
        main { class: "container",
            ProjectTabs { projects, current, page, new_project }
            NewProjectDialog { projects, current, open: new_project }
            Recipes { current, page, resource, new_recipe }
            List { page, resource, task_updates }
        }
    }
}

#[component]
fn ProjectTabs(
    projects: Resource<Vec<project::Project>>,
    current: Signal<i32>,
    page: Signal<i32>,
    new_project: Signal<bool>,
) -> Element {
    let list = projects().unwrap_or_default();
    rsx! {
        nav { class: "tabs",
            ul {
                for project in list.iter() {
                    li { key: "{project.id}",
                        button {
                            class: if current() == project.id { "tab active" } else { "tab" },
                            title: "{project.path}",
                            onclick: {
                                let id = project.id;
                                move |_| {
                                    select_project(current, id);
                                    page.set(1);
                                }
                            },
                            "{project.name}"
                        }
                    }
                }
            }
            ul {
                li {
                    button {
                        class: "outline new-button",
                        onclick: move |_| new_project.set(true),
                        "+ New Project"
                    }
                }
            }
        }
    }
}

/// A labelled text input for the dialog forms.
#[component]
fn Field(name: String, label: String, placeholder: String) -> Element {
    rsx! {
        label { r#for: "{name}", "{label}" }
        input {
            r#type: "text",
            name: "{name}",
            id: "{name}",
            placeholder: "{placeholder}",
            required: true,
        }
    }
}

/// Modal form shared by the dialogs, `on_submit` receives the submitted fields.
#[component]
fn DialogForm(
    title: String,
    open: Signal<bool>,
    error: Signal<String>,
    on_submit: EventHandler<Rc<FormData>>,
    children: Element,
) -> Element {
    if !open() {
        return rsx! {};
    }
    rsx! {
        dialog { open: true,
            article {
                header { h4 { "{title}" } }
                form {
                    onsubmit: move |evt| {
                        evt.prevent_default();
                        on_submit.call(evt.data.clone());
                    },
                    {children}
                    if !error().is_empty() {
                        small { style: "color: var(--pico-del-color)", "{error}" }
                    }
                    footer {
                        button {
                            r#type: "button",
                            class: "secondary",
                            onclick: move |_| {
                                error.set(String::new());
                                open.set(false);
                            },
                            "Cancel"
                        }
                        input { r#type: "submit", value: "Create" }
                    }
                }
            }
        }
    }
}

#[component]
fn NewProjectDialog(
    projects: Resource<Vec<project::Project>>,
    current: Signal<i32>,
    open: Signal<bool>,
) -> Element {
    let mut error = use_signal(String::new);
    rsx! {
        DialogForm {
            title: "New Project",
            open,
            error,
            on_submit: move |data: Rc<FormData>| {
                spawn(async move {
                    let created = create_project(
                        &form_value(&data, "name"),
                        &form_value(&data, "path"),
                        &form_value(&data, "output"),
                    )
                    .await;
                    match created {
                        Ok(created) => {
                            select_project(current, created.id);
                            projects.restart();
                            error.set(String::new());
                            open.set(false);
                        }
                        Err(message) => error.set(message),
                    }
                });
            },
            Field { name: "name", label: "Name", placeholder: "InnoProjector" }
            Field { name: "path", label: "Work directory", placeholder: "D:/InnoProjector" }
            Field {
                name: "output",
                label: "Output directory",
                placeholder: "D:/InnoProjector/Package",
            }
        }
    }
}

#[component]
fn Recipes(
    current: Signal<i32>,
    page: Signal<i32>,
    resource: Resource<Result<(Vec<task::Task>, i32), reqwest::Error>>,
    new_recipe: Signal<bool>,
) -> Element {
    let info = use_resource(move || async move {
        get_json::<project::ProjectInfo>(format!("/project/{}", current())).await
    });
    let (_, recipes) = info().unwrap_or_default();
    rsx! {
        nav { class: "recipes",
            ul {
                for recipe in recipes.iter() {
                    li { key: "{recipe.id}",
                        button {
                            class: "outline",
                            onclick: {
                                let recipe = recipe.clone();
                                move |_| {
                                    let recipe = recipe.clone();
                                    async move {
                                        run_recipe(&recipe, current()).await;
                                        page.set(1);
                                        resource.restart();
                                    }
                                }
                            },
                            "{recipe.name}"
                        }
                    }
                }
            }
            ul {
                li {
                    button {
                        class: "outline new-button",
                        onclick: move |_| new_recipe.set(true),
                        "+ New Recipe"
                    }
                }
            }
        }
        NewRecipeDialog { current, open: new_recipe, info }
    }
}

#[component]
fn NewRecipeDialog(
    current: Signal<i32>,
    open: Signal<bool>,
    info: Resource<project::ProjectInfo>,
) -> Element {
    let mut error = use_signal(String::new);
    rsx! {
        DialogForm {
            title: "New Recipe",
            open,
            error,
            on_submit: move |data: Rc<FormData>| {
                spawn(async move {
                    let created = create_recipe(
                        current(),
                        &form_value(&data, "name"),
                        &form_value(&data, "command"),
                    )
                    .await;
                    match created {
                        Ok(_) => {
                            error.set(String::new());
                            open.set(false);
                            info.restart();
                        }
                        Err(message) => error.set(message),
                    }
                });
            },
            Field { name: "name", label: "Name", placeholder: "update" }
            Field { name: "command", label: "Command", placeholder: "zip flir" }
        }
    }
}

#[component]
fn List(
    page: Signal<i32>,
    resource: Resource<Result<(Vec<task::Task>, i32), reqwest::Error>>,
    task_updates: Signal<HashMap<i32, String>>,
) -> Element {
    let updates = task_updates();

    match &*resource.read_unchecked() {
        Some(Ok((tasks, pages))) => {
            // Create updated task list with SSE status updates
            let updated_tasks: Vec<task::Task> = tasks
                .iter()
                .map(|task| {
                    let mut updated = task.clone();
                    if let Some(status) = updates.get(&task.id) {
                        updated.status = status.clone();
                    }
                    updated
                })
                .collect();

            rsx! {
                details { open: true,
                    summary { "Task List" }
                    table { class: "striped",
                        thead {
                            tr {
                                th { "ID" }
                                th { "Name" }
                                th { "Output" }
                                th { "Status" }
                                th { "" }
                            }
                        }
                        tbody {
                            for (id , task) in task::enumerate_tasks(&updated_tasks) {
                                tr { key: "{id}",
                                    td {
                                        a { href: "/logs/{task.month()}/{task.id}.log",
                                            "{task.id}"
                                        }
                                    }
                                    td { "{task.name}" }
                                    td {
                                        if let Some(output) = &task.output {
                                            if task.status == "Success" {
                                                a { href: "/package/{output}", "{task.filename()}" }
                                            } else {
                                                "{task.filename()}"
                                            }
                                        }
                                    }
                                    td { "{task.status_emoji()}" }
                                    td {
                                        if task.status == "Pending" {
                                            button {
                                                class: "outline secondary",
                                                onclick: move |_| async move {
                                                    reqwest::Client::new()
                                                        .post(format!("{}/cancel/{}", origin(), id))
                                                        .send()
                                                        .await
                                                        .unwrap();
                                                    resource.restart();
                                                },
                                                "Cancel"
                                            }
                                        } else if task.status == "Failed" && task.can_rerun() {
                                            button {
                                                class: "outline secondary",
                                                onclick: move |_| async move {
                                                    reqwest::Client::new()
                                                        .post(format!("{}/reset/{}", origin(), id))
                                                        .send()
                                                        .await
                                                        .unwrap();
                                                    resource.restart();
                                                },
                                                "Rerun"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    nav {
                        ul {
                            li {
                                button {
                                    class: "secondary",
                                    onclick: move |_| resource.restart(),
                                    "Refresh"
                                }
                            }
                        }
                        ul {
                            li {
                                button {
                                    class: "outline secondary contrast",
                                    onclick: move |_| page.set(page() - 1),
                                    disabled: page() == 1,
                                    "Prev"
                                }
                            }
                            span { "Page {page()}" }
                            li {
                                button {
                                    class: "outline secondary contrast",
                                    onclick: move |_| page.set(page() + 1),
                                    disabled: page() == *pages,
                                    "Next"
                                }
                            }
                        }
                    }
                }
            }
        }
        Some(Err(err)) => rsx! {
            div { "Loading tasks failed: {err}" }
        },
        None => rsx! {
            div { "Loading tasks..." }
        },
    }
}

fn origin() -> String {
    window().unwrap().location().origin().unwrap()
}

/// Project id from the `?project=` query string of the address bar.
fn query_project() -> Option<i32> {
    let search = window()?.location().search().ok()?;
    search
        .trim_start_matches('?')
        .split('&')
        .find_map(|pair| pair.strip_prefix("project="))
        .and_then(|value| value.parse().ok())
}

/// Select a project and mirror it in the address bar, so a refresh keeps it.
fn select_project(mut current: Signal<i32>, id: i32) {
    current.set(id);
    if let Some(history) = window().and_then(|window| window.history().ok()) {
        let url = format!("?project={id}");
        let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&url));
    }
}

/// GET a path on the server, falling back to the default value on any error.
async fn get_json<T: DeserializeOwned + Default>(path: impl Into<String>) -> T {
    reqwest::Client::new()
        .get(format!("{}{}", origin(), path.into()))
        .send()
        .await
        .unwrap()
        .json::<T>()
        .await
        .unwrap_or_default()
}

async fn create_project(name: &str, path: &str, output: &str) -> Result<project::Project, String> {
    post_json(
        "/projects",
        json!({ "name": name, "path": path, "output": output }),
    )
    .await
}

async fn create_recipe(project: i32, name: &str, command: &str) -> Result<recipe::Recipe, String> {
    post_json(
        "/recipes",
        json!({ "project": project, "name": name, "command": command }),
    )
    .await
}

/// POST a JSON body and return the created item, or the error message of the server.
async fn post_json<T: DeserializeOwned>(path: &str, body: serde_json::Value) -> Result<T, String> {
    let response = reqwest::Client::new()
        .post(format!("{}{path}", origin()))
        .json(&body)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if response.status().is_success() {
        response.json().await.map_err(|err| err.to_string())
    } else {
        Err(response.text().await.unwrap_or_default())
    }
}

fn form_value(data: &FormData, key: &str) -> String {
    data.values()
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| match value {
            dioxus::html::FormValue::Text(text) => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

async fn run_recipe(recipe: &recipe::Recipe, project: i32) {
    let _ = reqwest::Client::new()
        .post(format!("{}/run", origin()))
        .json(&json!({
            "name": recipe.name,
            "command": recipe.command,
            "project": project,
        }))
        .send()
        .await;
}

async fn connect_sse(mut task_updates: Signal<HashMap<i32, String>>) {
    use web_sys::EventSource;

    let sse_url = format!("{}/status", origin());

    // Track connection failures using a shared cell
    let failure_count = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let failure_count_clone = failure_count.clone();

    if let Ok(event_source) = EventSource::new(&sse_url) {
        let ontask_status = move |event: web_sys::MessageEvent| {
            // Reset failure count on successful message
            failure_count_clone.set(0);

            web_sys::console::log_1(
                &format!(
                    "Event received: {}",
                    event.data().as_string().unwrap_or_default()
                )
                .into(),
            );
            if let Some(data) = event.data().as_string() {
                // Parse the SSE event data
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                    if let (Some(task_id), Some(status)) = (
                        json.get("task_id")
                            .and_then(|v| v.as_i64())
                            .map(|v| v as i32),
                        json.get("status").and_then(|v| v.as_str()),
                    ) {
                        // Update the signal with the new task status
                        task_updates.with_mut(|updates| {
                            updates.insert(task_id, status.to_string());
                        });

                        // Log for debugging
                        web_sys::console::log_1(
                            &format!("Task {} status updated to: {}", task_id, status).into(),
                        );
                    }
                }
            }
        };

        // Listen for the 'task_status' event type specifically
        let closure = wasm_bindgen::prelude::Closure::wrap(
            Box::new(ontask_status) as Box<dyn FnMut(web_sys::MessageEvent)>
        );
        event_source
            .add_event_listener_with_callback("task_status", closure.as_ref().unchecked_ref())
            .expect("Failed to add event listener");
        closure.forget();

        // Add error handler to detect connection failures
        let event_source_clone = event_source.clone();
        let onerror = move |_event: web_sys::Event| {
            let failures = failure_count.get() + 1;
            failure_count.set(failures);
            web_sys::console::log_1(&format!("SSE connection error (attempt {})", failures).into());

            // If we've had too many failures, close the connection
            if failures > 10 {
                web_sys::console::log_1(
                    &"Stopping SSE reconnection attempts after 10 failures".into(),
                );
                event_source_clone.close();
            }
        };

        let error_closure = wasm_bindgen::prelude::Closure::wrap(
            Box::new(onerror) as Box<dyn FnMut(web_sys::Event)>
        );
        event_source.set_onerror(Some(error_closure.as_ref().unchecked_ref()));
        error_closure.forget();
    }
}
