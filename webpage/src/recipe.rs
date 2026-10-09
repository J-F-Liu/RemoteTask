use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Recipe {
    pub id: i32,
    pub name: String,
    pub command: String,
}
