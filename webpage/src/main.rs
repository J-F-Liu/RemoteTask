use dioxus::prelude::*;
use serde_json::json;
use time::OffsetDateTime;
use web_sys::window;

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
        p { "版本列表" }
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

    let mut name = String::new();
    name.push_str("A4");
    if control_card.as_str() == "hushu_dtk" {
        name.push_str("+");
    }
    name.push_str("、");
    name.push_str(&os_type);
    name.push_str("、");
    name.push_str(match package_type.as_str() {
        "setup" => "安装包",
        _ => "压缩包",
    });

    let now = OffsetDateTime::now_utc();
    let (year, month, day) = (now.year(), now.month() as u8, now.day());
    let card_type = match control_card.as_str() {
        "hushu_dtk" => "-N",
        _ => "",
    };
    let package_type = match package_type.as_str() {
        "setup" => "-Setup",
        _ => "",
    };
    let os_type = match os_type.as_str() {
        "Win10" => "-Win10",
        _ => "",
    };
    let output = format!(
        "Package/{year}-{month:02}/InnoProjector{package_type}{os_type}-{year}{month:02}{day:02}{card_type}.zip"
    );
    // document::eval(&format!("console.log(\"{}\");", output));

    let origin = window().unwrap().location().origin().unwrap();
    let client = reqwest::Client::new();
    let _res = client
        .post(format!("{}/run", origin))
        .json(&json!({
           "name": name,
           "command": command,
           "output": output,
        }))
        .send()
        .await?
        .text()
        .await?;
    Ok(())
}
