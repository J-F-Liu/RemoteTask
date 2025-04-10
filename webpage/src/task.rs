use serde::Deserialize;
use time::OffsetDateTime;

#[derive(Deserialize)]
pub struct Task {
    pub id: i32,
    pub name: String,
    // pub command: String,
    pub output: Option<String>,
    pub status: String,
    pub created_at: String,
    // pub updated_at: String,
}

impl Task {
    pub fn month(&self) -> String {
        self.created_at[..7].to_string()
    }

    pub fn date(&self) -> String {
        self.created_at[..10].to_string()
    }

    pub fn status_emoji(&self) -> &'static str {
        match self.status.as_str() {
            "Pending" => "⏳",
            "Running" => "🏗️",
            "Success" => "✅",
            "Failed" => "❌",
            _ => "❓",
        }
    }

    pub fn filename(&self) -> &str {
        if let Some(output) = &self.output {
            if let Some((_, filename)) = output.rsplit_once('/') {
                return filename;
            } else {
                return output;
            }
        }
        ""
    }

    pub fn can_rerun(&self) -> bool {
        today() == self.date()
    }
}

pub fn enumerate_tasks(tasks: &[Task]) -> impl Iterator<Item = (i32, &Task)> {
    let ids = tasks.iter().map(|task| task.id).collect::<Vec<_>>();
    ids.into_iter().zip(tasks.iter())
}

pub fn today() -> String {
    let now = OffsetDateTime::now_utc();
    format!("{}-{:02}-{:02}", now.year(), now.month() as u8, now.day())
}
