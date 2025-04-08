use serde::Deserialize;

#[derive(Deserialize)]
pub struct Task {
    pub id: i32,
    pub name: String,
    // pub command: String,
    pub output: Option<String>,
    pub status: String,
    // pub created_at: String,
    // pub updated_at: String,
}

impl Task {
    // pub fn month(&self) -> String {
    //     let year = self.created_at[..4].parse::<i32>().unwrap();
    //     let month = self.created_at[5..7].parse::<u8>().unwrap();
    //     format!("{year}-{month:02}")
    // }

    pub fn status_emoji(&self) -> &'static str {
        match self.status.as_str() {
            "Pending" => "⏳",
            "Running" => "🏗️",
            "Success" => "✅",
            "Failed" => "❌",
            _ => "❓",
        }
    }
}

pub fn enumerate_tasks(tasks: &[Task]) -> impl Iterator<Item = (i32, &Task)> {
    let ids = tasks.iter().map(|task| task.id).collect::<Vec<_>>();
    ids.into_iter().zip(tasks.iter())
}
