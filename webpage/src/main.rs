use dioxus::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use wasm_bindgen::JsCast;
use web_sys::window;

const PICO_CSS: Asset = asset!("/assets/pico.min.css");
const MAIN_CSS: Asset = asset!("/assets/main.css");

mod project;
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
    let mut current = use_signal(|| 0i32);
    let new_project = use_signal(|| false);
    let task_updates = use_signal(HashMap::<i32, String>::new);

    let projects = use_resource(move || async move { fetch_projects().await });

    // Select the first project once the list is loaded
    use_effect(move || {
        if current() == 0 {
            if let Some(id) = projects().and_then(|list| list.first().map(|p| p.id)) {
                current.set(id);
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
            Form { page, resource, current }
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
                                    current.set(id);
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
                        class: "outline",
                        onclick: move |_| new_project.set(true),
                        "+ New Project"
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
    if !open() {
        return rsx! {};
    }
    rsx! {
        dialog { open: true,
            article {
                header { h4 { "New Project" } }
                form {
                    onsubmit: move |evt| async move {
                        evt.prevent_default();
                        let created = create_project(
                            &form_value(&evt.data, "name"),
                            &form_value(&evt.data, "path"),
                            &form_value(&evt.data, "output"),
                        )
                        .await;
                        match created {
                            Ok(created) => {
                                current.set(created.id);
                                projects.restart();
                                error.set(String::new());
                                open.set(false);
                            }
                            Err(message) => error.set(message),
                        }
                    },
                    label { r#for: "name", "Name" }
                    input {
                        r#type: "text",
                        name: "name",
                        id: "name",
                        placeholder: "InnoProjector",
                        required: true,
                    }
                    label { r#for: "path", "Work directory" }
                    input {
                        r#type: "text",
                        name: "path",
                        id: "path",
                        placeholder: "D:/InnoProjector",
                        required: true,
                    }
                    label { r#for: "output", "Output directory" }
                    input {
                        r#type: "text",
                        name: "output",
                        id: "output",
                        placeholder: "D:/InnoProjector/Package",
                        required: true,
                    }
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
fn Form(
    page: Signal<i32>,
    resource: Resource<Result<(Vec<task::Task>, i32), reqwest::Error>>,
    current: Signal<i32>,
) -> Element {
    let info = use_resource(move || async move {
        reqwest::Client::new()
            .get(format!("{}/project/{}", origin(), current()))
            .send()
            .await
            .unwrap()
            .json::<(project::Project, Vec<String>)>()
            .await
            .unwrap_or_default()
    });
    let (_, recipes) = info().unwrap_or_default();
    rsx! {
        form {
            class: "grid",
            onsubmit: move |evt| async move {
                evt.prevent_default();
                submit_form(&evt.data, current()).await.unwrap();
                page.set(1);
                resource.restart();
            },
            fieldset { role: "group", class: "gc1-4",
                input {
                    r#type: "text",
                    name: "task",
                    id: "task",
                    value: "",
                    list: "task-list",
                }
                datalist { id: "task-list",
                    for (index , recipe) in recipes.iter().enumerate() {
                        option { id: index, value: "{recipe}" }
                    }
                }
                input { r#type: "submit", value: "Run Task" }
            }
        }
        hr {}
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

async fn fetch_projects() -> Vec<project::Project> {
    reqwest::Client::new()
        .get(format!("{}/projects", origin()))
        .send()
        .await
        .unwrap()
        .json::<Vec<project::Project>>()
        .await
        .unwrap_or_default()
}

async fn create_project(name: &str, path: &str, output: &str) -> Result<project::Project, String> {
    let response = reqwest::Client::new()
        .post(format!("{}/projects", origin()))
        .json(&json!({ "name": name, "path": path, "output": output }))
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

async fn submit_form(data: &FormData, project: i32) -> Result<(), reqwest::Error> {
    let task = form_value(data, "task");
    let name = task.split(' ').next().unwrap_or_default();

    let _res = reqwest::Client::new()
        .post(format!("{}/run", origin()))
        .json(&json!({
           "name": name,
           "command": task,
           "project": (project > 0).then_some(project),
        }))
        .send()
        .await?
        .text()
        .await?;
    Ok(())
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
