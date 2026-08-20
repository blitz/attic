//! A build trace entry.

use sea_orm::entity::prelude::*;

use super::Json;
use attic::api::v1::build_trace::BuildTraceSignature;

pub type BuildTraceModel = Model;

/// A mapping from a derivation output to its realized store path.
#[derive(Debug, Clone, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "build_trace")]
pub struct Model {
    /// Unique numeric ID of the entry.
    #[sea_orm(primary_key)]
    pub id: i64,

    /// ID of the cache the entry belongs to.
    #[sea_orm(indexed)]
    pub cache_id: i64,

    /// Base name of the producing derivation.
    ///
    /// For example `<hash>-hello.drv`. Nix looks the output up by the resolved
    /// derivation, so that is what this column holds.
    #[sea_orm(column_type = "String(StringLen::N(255))", indexed)]
    pub drv_path: String,

    /// Name of the derivation output, such as `out` or `dev`.
    #[sea_orm(column_type = "String(StringLen::N(128))")]
    pub output_name: String,

    /// Base name of the realized store path.
    #[sea_orm(column_type = "String(StringLen::N(255))")]
    pub out_path: String,

    /// The hash portion of `out_path`.
    ///
    /// Denormalized so joins against the object table need no backend-specific
    /// substring expression. Those joins drive off the object table's index.
    #[sea_orm(column_type = "String(StringLen::N(32))")]
    pub out_path_hash: String,

    /// Signatures over the entry.
    ///
    /// Because Nix does not yet verify these when substituting, this will be empty.
    pub signatures: Json<Vec<BuildTraceSignature>>,

    /// Timestamp of entry creation.
    pub created_at: ChronoDateTimeUtc,

    /// The uploader of the entry.
    ///
    /// This is a "username", set to the `sub` claim in the client's JWT.
    pub created_by: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::cache::Entity",
        from = "Column::CacheId",
        to = "super::cache::Column::Id"
    )]
    Cache,
}

impl Related<super::cache::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Cache.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
