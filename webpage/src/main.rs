use dioxus::prelude::*;
use serde_json::json;
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
    rsx! {
        main { class: "container",
            Form { page, resource }
            List { page, resource }
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
                evt.stop_propagation();
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
) -> Element {
    match &*resource.read_unchecked() {
        Some(Ok((tasks, pages))) => rsx! {
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
                        for (id , task) in task::enumerate_tasks(tasks) {
                            tr { key: id,
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
        },
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
    let task = values.get("task").unwrap().as_value();
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
