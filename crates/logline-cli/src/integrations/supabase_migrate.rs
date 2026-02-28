use crate::commands::db::{DbCommands, MigrateCommands};

/// Run migrations via the db module.
pub fn migrate_up(env: &str, json: bool) -> anyhow::Result<()> {
    crate::commands::db::cmd_db(
        DbCommands::Migrate {
            command: MigrateCommands::Up {
                env: env.to_string(),
            },
        },
        json,
    )
}

/// Run RLS verification gate.
pub fn verify_rls(env: &str, json: bool) -> anyhow::Result<()> {
    crate::commands::db::cmd_db(
        DbCommands::VerifyRls {
            env: env.to_string(),
        },
        json,
    )
}
