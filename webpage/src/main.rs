use dioxus::prelude::*;
use serde_json::json;

const FAVICON: Asset = asset!("/assets/favicon.ico");
const PICO_CSS: Asset = asset!("/assets/pico.min.css");
const MAIN_CSS: Asset = asset!("/assets/main.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
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
            h1 { "InnoProjector 版本发布系统" }
            hr {}
        }
    }
}

#[component]
fn Main() -> Element {
    rsx! {
        main { class: "container",
            Form {}
            List {}
        }
    }
}

#[component]
fn Form() -> Element {
    rsx! {
        form {
            class: "grid",
            onsubmit: move |evt| {
                evt.stop_propagation();
                spawn(async move { submit_form(&evt.data).await.unwrap() });
            },
            fieldset {
                legend { "控制卡型号" }
                input {
                    r#type: "radio",
                    name: "control-card",
                    id: "A4",
                    value: "",
                    checked: true,
                }
                label { r#for: "A4", "A4" }
                input {
                    r#type: "radio",
                    name: "control-card",
                    id: "A4plus",
                    value: "hushu_dtk",
                }
                label { r#for: "A4plus", "A4 + 串口" }
            }
            fieldset {
                legend { "操作系统" }
                input {
                    r#type: "radio",
                    name: "os-type",
                    id: "Win10",
                    value: "Win10",
                }
                label { r#for: "Win10", "Win10" }
                input {
                    r#type: "radio",
                    name: "os-type",
                    id: "Win11",
                    value: "Win11",
                    checked: true,
                }
                label { r#for: "Win11", "Win11" }
            }
            fieldset {
                legend { "打包类型" }
                input {
                    r#type: "radio",
                    name: "package-type",
                    id: "zip",
                    value: "zip",
                    checked: true,
                }
                label { r#for: "zip", "压缩包" }
                input {
                    r#type: "radio",
                    name: "package-type",
                    id: "setup",
                    value: "setup",
                }
                label { r#for: "setup", "安装包" }
            }
            input { r#type: "submit", value: "生成新版本" }
        }
        hr {}
    }
}

#[component]
fn List() -> Element {
    rsx! {
        p { "任务列表" }
        ul { class: "list",
            li { "任务 1" }
            li { "任务 2" }
            li { "任务 3" }
        }
    }
}

async fn submit_form(data: &FormData) -> Result<(), reqwest::Error> {
    let values = data.values();
    let control_card = values.get("control-card").unwrap().as_value();
    let os_type = values.get("os-type").unwrap().as_value();
    let package_type = values.get("package-type").unwrap().as_value();

    let command = format!("build_{os_type}_{package_type} {control_card}");
    document::eval(&format!("console.log(\"{}\");", command));

    let client = reqwest::Client::new();
    let response = client
        .post("http://127.0.0.1:8080/run")
        .json(&json!({
           "name": "Build InnoProjector",
           "command": command,
           "output": "output.txt",
        }))
        .send()
        .await?;
    Ok(())
}
