use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Project {
    pub id: i32,
    pub name: String,
    pub path: String,
}
