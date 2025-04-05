use sea_orm::DbConn;
use sea_orm::{Set, entity::prelude::*};

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "task")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub command: String,
    pub output: Option<String>,
    pub status: TaskStatus,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[derive(Clone, Debug, PartialEq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(1))")]
pub enum TaskStatus {
    #[sea_orm(string_value = "P")]
    Pending,
    #[sea_orm(string_value = "R")]
    Running,
    #[sea_orm(string_value = "S")]
    Success,
    #[sea_orm(string_value = "F")]
    Failed,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

pub async fn create_task(db: &DbConn, name: String, command: String) -> Result<ActiveModel, DbErr> {
    ActiveModel {
        name: Set(name),
        command: Set(command),
        status: Set(TaskStatus::Pending),
        created_at: Set(TimeDateTimeWithTimeZone::now_utc()),
        updated_at: Set(TimeDateTimeWithTimeZone::now_utc()),
        ..Default::default()
    }
    .save(db)
    .await
}

pub async fn update_task(db: &DbConn, id: i32, status: TaskStatus) -> Result<Model, DbErr> {
    let task: ActiveModel = Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or(DbErr::Custom("Cannot find post.".to_owned()))
        .map(Into::into)?;

    ActiveModel {
        id: task.id,
        status: Set(status),
        ..Default::default()
    }
    .update(db)
    .await
}
