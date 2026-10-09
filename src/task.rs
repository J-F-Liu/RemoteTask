use sea_orm::{
    DbConn, QueryOrder, Set, TryIntoModel, Unchanged, entity::prelude::*, sea_query::Expr,
};
use serde::Serialize;

#[derive(Clone, Debug, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "task")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub dir: String,
    pub command: String,
    pub output: Option<String>,
    pub project_id: i32,
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

pub async fn create_task(
    db: &DbConn,
    dir: String,
    name: String,
    command: String,
    output: Option<String>,
    project_id: i32,
) -> Result<Model, DbErr> {
    let now = TimeDateTimeWithTimeZone::now_utc();
    ActiveModel {
        name: Set(name),
        dir: Set(dir),
        command: Set(command),
        output: Set(output),
        project_id: Set(project_id),
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
    project: Option<i32>,
) -> Result<(Vec<Model>, u64), DbErr> {
    let mut query = Entity::find();
    if let Some(project) = project {
        query = query.filter(Column::ProjectId.eq(project));
    }
    let paginator = query.order_by_desc(Column::Id).paginate(db, page_size);
    let pages = paginator.num_pages().await?;
    let items = paginator.fetch_page(page).await?;
    Ok((items, pages))
}

/// Move tasks that don't belong to any project yet to the given project.
pub async fn assign_unassigned(db: &DbConn, project_id: i32) -> Result<(), DbErr> {
    Entity::update_many()
        .col_expr(Column::ProjectId, Expr::value(project_id))
        .filter(Column::ProjectId.eq(0))
        .exec(db)
        .await
        .map(|_| ())
}
