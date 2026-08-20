use sea_orm_migration::prelude::*;

use crate::database::entity::build_trace::*;
use crate::database::entity::cache;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260817_000001_add_build_trace_table"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Entity)
                    .col(
                        ColumnDef::new(Column::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Column::CacheId).big_integer().not_null())
                    // Bounded because PostgreSQL caps a btree index tuple at
                    // 2704 bytes.
                    .col(ColumnDef::new(Column::DrvPath).string_len(255).not_null())
                    .col(
                        ColumnDef::new(Column::OutputName)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Column::OutPath).string_len(255).not_null())
                    .col(
                        ColumnDef::new(Column::OutPathHash)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Column::Signatures).string().not_null())
                    .col(
                        ColumnDef::new(Column::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Column::CreatedBy).string())
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .name("fk_build_trace_cache")
                            .from_tbl(Entity)
                            .from_col(Column::CacheId)
                            .to_tbl(cache::Entity)
                            .to_col(cache::Column::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Unique because a derivation output resolves to exactly one store path
        // per cache.
        manager
            .create_index(
                Index::create()
                    .name("idx-build-trace-lookup")
                    .table(Entity)
                    .col(Column::CacheId)
                    .col(Column::DrvPath)
                    .col(Column::OutputName)
                    .unique()
                    .to_owned(),
            )
            .await
    }
}
