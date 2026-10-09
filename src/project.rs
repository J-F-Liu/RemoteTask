use sea_orm::{DbConn, QueryOrder, Set, TryIntoModel, entity::prelude::*};
use serde::Serialize;

#[derive(Clone, Debug, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "project")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique)]
    pub name: String,
    pub path: String,
    pub output: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

pub async fn list(db: &DbConn) -> Result<Vec<Model>, DbErr> {
    Entity::find().order_by_asc(Column::Id).all(db).await
}

pub async fn find(db: &DbConn, id: i32) -> Result<Option<Model>, DbErr> {
    Entity::find_by_id(id).one(db).await
}

pub async fn find_by_name(db: &DbConn, name: &str) -> Result<Option<Model>, DbErr> {
    Entity::find().filter(Column::Name.eq(name)).one(db).await
}

pub async fn create(
    db: &DbConn,
    name: String,
    path: String,
    output: String,
) -> Result<Model, DbErr> {
    ActiveModel {
        name: Set(name),
        path: Set(path),
        output: Set(output),
        ..Default::default()
    }
    .save(db)
    .await
    .and_then(|model| model.try_into_model())
}
