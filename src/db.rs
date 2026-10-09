use crate::{project, recipe, task};
use sea_orm::{ConnectionTrait, DbConn, DbErr, EntityTrait, PaginatorTrait, Schema, Statement};
use std::path::Path;

/// Create missing tables and columns, then create the first project from the environment.
pub async fn migrate(db: &DbConn, work_dir: &Path, output_dir: &Path) -> Result<(), DbErr> {
    create_table::<task::Entity>(db).await?;
    create_table::<project::Entity>(db).await?;
    create_table::<recipe::Entity>(db).await?;
    add_column_if_missing(db, "task", "dir", "TEXT", "''").await?;
    add_column_if_missing(db, "task", "project_id", "INTEGER", "0").await?;
    seed_project(db, work_dir, output_dir).await
}

async fn create_table<E: EntityTrait>(db: &DbConn) -> Result<(), DbErr> {
    let backend = db.get_database_backend();
    let mut statement = Schema::new(backend).create_table_from_entity(E::default());
    db.execute(backend.build(statement.if_not_exists())).await?;
    Ok(())
}

async fn add_column_if_missing(
    db: &DbConn,
    table_name: &str,
    column_name: &str,
    column_type: &str,
    column_default: &str,
) -> Result<(), DbErr> {
    let backend = db.get_database_backend();
    let sql =
        Statement::from_sql_and_values(backend, format!("PRAGMA table_info({table_name})"), vec![]);
    let column_exists = db
        .query_all(sql)
        .await?
        .iter()
        .any(|row| row.try_get::<String>("", "name") == Ok(column_name.to_string()));
    if !column_exists {
        let sql = Statement::from_sql_and_values(
            backend,
            format!(
                "ALTER TABLE {table_name} ADD COLUMN {column_name} {column_type} NOT NULL DEFAULT {column_default}"
            ),
            vec![],
        );
        db.execute(sql).await?;
    }
    Ok(())
}

/// Create the project of the configured work directory when there is none yet.
async fn seed_project(db: &DbConn, path: &Path, output: &Path) -> Result<(), DbErr> {
    if project::Entity::find().count(db).await? > 0 {
        return Ok(());
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Default")
        .to_string();
    let project = project::create(
        db,
        name,
        path.to_string_lossy().into_owned(),
        output.to_string_lossy().into_owned(),
    )
    .await?;
    // Tasks created before the project table existed belong to the first project.
    task::assign_unassigned(db, project.id).await
}
