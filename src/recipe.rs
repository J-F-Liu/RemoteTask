use sea_orm::{DbConn, QueryOrder, Set, TryIntoModel, Unchanged, entity::prelude::*};
use serde::Serialize;

#[derive(Clone, Debug, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "recipe")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub project_id: i32,
    pub name: String,
    pub command: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

pub async fn list(db: &DbConn, project_id: i32) -> Result<Vec<Model>, DbErr> {
    Entity::find()
        .filter(Column::ProjectId.eq(project_id))
        .order_by_asc(Column::Id)
        .all(db)
        .await
}

/// Create the recipe, or update the command of the recipe with the same name.
pub async fn upsert(
    db: &DbConn,
    project_id: i32,
    name: String,
    command: String,
) -> Result<Model, DbErr> {
    let existing = Entity::find()
        .filter(Column::ProjectId.eq(project_id))
        .filter(Column::Name.eq(name.as_str()))
        .one(db)
        .await?;
    match existing {
        Some(recipe) => {
            ActiveModel {
                id: Unchanged(recipe.id),
                command: Set(command),
                ..Default::default()
            }
            .update(db)
            .await
        }
        None => ActiveModel {
            project_id: Set(project_id),
            name: Set(name),
            command: Set(command),
            ..Default::default()
        }
        .save(db)
        .await
        .and_then(|model| model.try_into_model()),
    }
}
