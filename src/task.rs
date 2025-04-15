use sea_orm::{DbConn, QueryOrder, Set, TryIntoModel, Unchanged, entity::prelude::*};
use serde::Serialize;

#[derive(Clone, Debug, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "task")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub command: String,
    pub output: Option<String>,
    pub status: TaskStatus,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: time::OffsetDateTime,
}

#[derive(Copy, Clone, Debug, PartialEq, EnumIter, DeriveActiveEnum, Serialize)]
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

impl Model {
    pub fn month(&self) -> String {
        let year = self.created_at.year();
        let month = self.created_at.month() as u8;
        format!("{year}-{month:02}")
    }
}

pub async fn create_table_if_not_exists(db: &DbConn) -> Result<(), DbErr> {
    let backend = db.get_database_backend();
    let schema = sea_orm::Schema::new(backend);
    let mut statement = schema.create_table_from_entity(Entity);
    let statement = backend.build(statement.if_not_exists());
    db.execute(statement).await.map(|_| ())
}

pub async fn create_task(
    db: &DbConn,
    name: String,
    command: String,
    output: Option<String>,
) -> Result<Model, DbErr> {
    let now = TimeDateTimeWithTimeZone::now_utc();
    ActiveModel {
        name: Set(name),
        command: Set(command),
        output: Set(output),
        status: Set(TaskStatus::Pending),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .save(db)
    .await
    .and_then(|m| m.try_into_model())
}

pub async fn update_task(db: &DbConn, id: i32, status: TaskStatus) -> Result<Model, DbErr> {
    let task: Model = Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or(DbErr::Custom("Cannot find task.".to_owned()))?;

    ActiveModel {
        id: Unchanged(task.id),
        status: Set(status),
        updated_at: Set(TimeDateTimeWithTimeZone::now_utc()),
        ..Default::default()
    }
    .update(db)
    .await
}

pub async fn delete_task(db: &DbConn, id: i32) -> Result<bool, DbErr> {
    Entity::delete_by_id(id)
        .exec(db)
        .await
        .map(|m| m.rows_affected == 1)
}

pub async fn pending_tasks(db: &DbConn) -> Result<Vec<Model>, DbErr> {
    Entity::find()
        .filter(Column::Status.eq(TaskStatus::Pending))
        .order_by_asc(Column::CreatedAt)
        .all(db)
        .await
}

pub async fn recent_tasks(
    db: &DbConn,
    page_size: u64,
    page: u64,
) -> Result<(Vec<Model>, u64), DbErr> {
    let paginator = Entity::find()
        .order_by_desc(Column::Id)
        .paginate(db, page_size);
    let pages = paginator.num_pages().await?;
    let items = paginator.fetch_page(page).await?;
    Ok((items, pages))
}
