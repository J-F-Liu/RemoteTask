use dioxus::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use wasm_bindgen::JsCast;
use web_sys::window;

const PICO_CSS: Asset = asset!("/assets/pico.min.css");
const MAIN_CSS: Asset = asset!("/assets/main.css");

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
    let task_updates = use_signal(|| HashMap::<i32, String>::new());
    let resource = use_resource(move || async move {
        let origin = window().unwrap().location().origin().unwrap();
        let client = reqwest::Client::new();
        client
            .get(format!("{}/list/{}", origin, page()))
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
            Form { page, resource }
            List { page, resource, task_updates }
        }
    }
}

#[component]
fn Form(
    page: Signal<i32>,
    resource: Resource<Result<(Vec<task::Task>, i32), reqwest::Error>>,
) -> Element {
    let recipes = use_resource(move || async move {
        let origin = window().unwrap().location().origin().unwrap();
        let client = reqwest::Client::new();
        client
            .get(format!("{}/menu", origin))
            .send()
            .await
            .unwrap()
            .json::<Vec<String>>()
            .await
            .unwrap_or_default()
    });
    rsx! {
        form {
            class: "grid",
            onsubmit: move |evt| async move {
                evt.prevent_default();
                submit_form(&evt.data).await.unwrap();
                page.set(1);
                resource.restart();
            },
            fieldset { role: "group", class: "gc1-4",
                label { "Select Task" }
                input {
                    r#type: "text",
                    name: "task",
                    id: "task",
                    value: "",
                    list: "task-list",
                }
                datalist { id: "task-list",
                    for (index , recipe) in recipes.read_unchecked().clone().unwrap_or(vec![]).iter().enumerate() {
                        option { id: index, value: "{recipe}" }
                    }
                }
            }
            input { r#type: "submit", value: "Run" }
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
                                                    let origin = window().unwrap().location().origin().unwrap();
                                                    let client = reqwest::Client::new();
                                                    client.post(format!("{}/cancel/{}", origin, id)).send().await.unwrap();
                                                    resource.restart();
                                                },
                                                "Cancel"
                                            }
                                        } else if task.status == "Failed" && task.can_rerun() {
                                            button {
                                                class: "outline secondary",
                                                onclick: move |_| async move {
                                                    let origin = window().unwrap().location().origin().unwrap();
                                                    let client = reqwest::Client::new();
                                                    client.post(format!("{}/reset/{}", origin, id)).send().await.unwrap();
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

async fn submit_form(data: &FormData) -> Result<(), reqwest::Error> {
    let values = data.values();
    let task = values
        .iter()
        .find(|(k, _)| k == "task")
        .and_then(|(_, v)| match v {
            dioxus::html::FormValue::Text(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let name = task.split(' ').next().unwrap_or_default();

    let origin = window().unwrap().location().origin().unwrap();
    let client = reqwest::Client::new();
    let _res = client
        .post(format!("{}/run", origin))
        .json(&json!({
           "name": name,
           "command": task,
        }))
        .send()
        .await?
        .text()
        .await?;
    Ok(())
}

async fn connect_sse(mut task_updates: Signal<HashMap<i32, String>>) {
    use web_sys::EventSource;

    let origin = window().unwrap().location().origin().unwrap();
    let sse_url = format!("{}/status", origin);

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
