use super::{
    catalog::{self, Catalog, DbForeignKey, DbIndex},
    ddl, error, DbColumn, Dialect, MigrationChange, MigrationPlan, SqlBackend, SqlError, SqlModel,
};

fn effective_default(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|s| *s != "null")
}
fn same_column(a: &DbColumn, b: &DbColumn) -> bool {
    a.name == b.name
        && a.kind == b.kind
        && a.nullable == b.nullable
        && a.primary == b.primary
        && a.auto == b.auto
        && effective_default(&a.default) == effective_default(&b.default)
}
fn same_fk(a: &DbForeignKey, b: &DbForeignKey) -> bool {
    a.column == b.column && a.table == b.table && a.target == b.target && a.on_delete == b.on_delete
}
fn has_index(catalog: &Catalog, index: &DbIndex) -> bool {
    catalog.indexes.iter().any(|i| {
        i.columns == index.columns && (i.unique == index.unique || i.unique && !index.unique)
    })
}
pub(super) fn matches(actual: &Catalog, target: &Catalog) -> bool {
    actual.exists
        && actual.columns.len() == target.columns.len()
        && target
            .columns
            .iter()
            .all(|c| actual.columns.iter().any(|a| same_column(a, c)))
        && target.indexes.iter().all(|i| has_index(actual, i))
        && actual.indexes.iter().all(|i| {
            has_index(target, i)
                || !i.unique
                    && target
                        .foreign_keys
                        .iter()
                        .any(|f| i.columns == [f.column.clone()])
        })
        && actual.foreign_keys.len() == target.foreign_keys.len()
        && target
            .foreign_keys
            .iter()
            .all(|f| actual.foreign_keys.iter().any(|a| same_fk(a, f)))
}
fn target<T: SqlModel, DB: SqlBackend>() -> Catalog {
    let mut target = Catalog {
        exists: true,
        ..Catalog::default()
    };
    for column in T::COLUMNS {
        target.columns.push(DbColumn {
            name: column.name.into(),
            kind: catalog::normalize_type(&ddl::declaration(column, DB::DIALECT), DB::DIALECT),
            nullable: column.nullable && !column.primary_key,
            primary: column.primary_key,
            auto: column.auto_increment,
            default: column.default.map(|d| d.canonical(DB::DIALECT)),
        });
        if !column.primary_key && (column.unique || column.index) {
            target.indexes.push(DbIndex {
                name: ddl::index_name::<T>(column),
                columns: vec![column.name.into()],
                unique: column.unique,
                constraint: false,
            });
        }
        if let Some(reference) = column.references {
            let source = format!(
                "{}_{}_{}_{}_{}_{}",
                T::TABLE.len(),
                T::TABLE,
                column.name,
                reference.table,
                reference.column,
                reference.on_delete.sql()
            );
            let hash = source.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
            let mut on_delete = reference.on_delete.sql();
            if DB::DIALECT == Dialect::MySql && on_delete == "NO ACTION" {
                on_delete = "RESTRICT";
            }
            target.foreign_keys.push(DbForeignKey {
                name: format!("rydantic_fk_{hash:016x}"),
                column: column.name.into(),
                table: reference.table.into(),
                target: reference.column.into(),
                on_delete: on_delete.into(),
            });
        }
    }
    target.columns.sort_by(|a, b| a.name.cmp(&b.name));
    target.indexes.sort_by(|a, b| a.name.cmp(&b.name));
    target.foreign_keys.sort_by(|a, b| a.column.cmp(&b.column));
    target
}
fn change(plan: &mut MigrationPlan, description: String, destructive: bool) {
    plan.changes.push(MigrationChange {
        description,
        destructive,
    });
}
fn widened(old: &str, new: &str) -> bool {
    ["VARCHAR(", "VARBINARY("].iter().any(|prefix| {
        let length = |s: &str| {
            s.strip_prefix(prefix)
                .and_then(|s| s.strip_suffix(')'))
                .and_then(|s| s.parse::<usize>().ok())
        };
        matches!((length(old),length(new)),(Some(a),Some(b)) if b>=a)
    }) || (old.starts_with("VARCHAR(") && new == "LONGTEXT")
        || (old.starts_with("VARBINARY(") && new == "LONGBLOB")
}
pub(super) fn build<T: SqlModel, DB: SqlBackend>(
    before: Catalog,
) -> Result<MigrationPlan, SqlError> {
    if T::TABLE.starts_with("_rydantic_") {
        return Err(error(
            "migration_reserved",
            "table names beginning with _rydantic_ are reserved for migration metadata",
        ));
    }
    let dialect = DB::DIALECT;
    let q = |name: &str| dialect.quote(name);
    let table = q(T::TABLE);
    let mut plan = MigrationPlan {
        table: T::TABLE.into(),
        dialect,
        before: before.clone(),
        target: target::<T, DB>(),
        changes: Vec::new(),
        statements: Vec::new(),
        rebuild: false,
        checks: Vec::new(),
    };
    if !before.exists {
        change(&mut plan, format!("Create table {}", T::TABLE), false);
        plan.statements.push(T::create_table_sql::<DB>());
        plan.statements.extend(T::create_index_sql::<DB>());
        return Ok(plan);
    }
    let mut current = before;
    let mut rename_sources = std::collections::HashSet::new();
    for column in T::COLUMNS {
        if current.columns.iter().any(|c| c.name == column.name) {
            continue;
        }
        if let Some(source) = column.rename_from {
            if plan.target.columns.iter().any(|c| c.name == source)
                || !rename_sources.insert(source)
            {
                return Err(error(
                    "migration_rename",
                    "rename_from must identify a distinct previous column",
                ));
            }
            if let Some(old) = current.columns.iter_mut().find(|c| c.name == source) {
                old.name = column.name.into();
                for index in &mut current.indexes {
                    for c in &mut index.columns {
                        if c == source {
                            *c = column.name.into();
                        }
                    }
                }
                for fk in &mut current.foreign_keys {
                    if fk.column == source {
                        fk.column = column.name.into();
                    }
                    if fk.table == T::TABLE && fk.target == source {
                        fk.target = column.name.into();
                    }
                }
                change(
                    &mut plan,
                    format!("Rename column {source} to {}", column.name),
                    false,
                );
                plan.statements.push(format!(
                    "ALTER TABLE {table} RENAME COLUMN {} TO {}",
                    q(source),
                    q(column.name)
                ));
            }
        }
    }
    for old in &current.columns {
        if !plan.target.columns.iter().any(|c| c.name == old.name) {
            if old.primary {
                return Err(error(
                    "migration_primary_key",
                    "automatic migration preserves the primary key; keep it or declare rename_from",
                ));
            }
            change(&mut plan, format!("Drop column {}", old.name), true);
            plan.rebuild = true;
        }
    }
    for wanted in plan.target.columns.clone() {
        if let Some(old) = current.columns.iter().find(|c| c.name == wanted.name) {
            if old.primary != wanted.primary || old.auto != wanted.auto {
                return Err(error(
                    "migration_primary_key",
                    "automatic migration preserves primary-key and identity strategy",
                ));
            }
            if !same_column(old, &wanted) {
                change(
                    &mut plan,
                    format!("Alter column {}", wanted.name),
                    old.kind != wanted.kind && !widened(&old.kind, &wanted.kind),
                );
                plan.rebuild = true;
            }
        } else {
            if wanted.primary {
                return Err(error(
                    "migration_primary_key",
                    "adding a primary key to an existing table requires an explicit data migration",
                ));
            }
            change(&mut plan, format!("Add column {}", wanted.name), false);
            plan.rebuild = true;
            if !wanted.nullable && wanted.default.is_none() {
                plan.checks.push((
                    format!("SELECT 1 FROM {table} LIMIT 1"),
                    format!(
                        "required column {} needs #[sql(default = ...)] to backfill existing rows",
                        wanted.name
                    ),
                ));
            }
        }
        if !wanted.nullable {
            if let Some(old) = current.columns.iter().find(|c| c.name == wanted.name) {
                if old.nullable && wanted.default.is_none() {
                    plan.checks.push((format!("SELECT 1 FROM {table} WHERE {} IS NULL LIMIT 1",q(&old.name)),format!("column {} contains NULL; provide a SQL default before making it required",old.name)));
                }
            }
        }
    }
    let removed_indexes = current
        .indexes
        .iter()
        .filter(|i| {
            !has_index(&plan.target, i)
                && !(dialect == Dialect::MySql
                    && !i.unique
                    && plan
                        .target
                        .foreign_keys
                        .iter()
                        .any(|f| i.columns == [f.column.clone()]))
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut retained = current.clone();
    retained.indexes.retain(|i| !removed_indexes.contains(i));
    let added_indexes = plan
        .target
        .indexes
        .iter()
        .filter(|i| !has_index(&retained, i))
        .cloned()
        .collect::<Vec<_>>();
    for index in &removed_indexes {
        change(
            &mut plan,
            format!(
                "Drop {}index {}",
                if index.unique { "unique " } else { "" },
                index.name
            ),
            false,
        );
    }
    for index in &added_indexes {
        change(
            &mut plan,
            format!(
                "Add {}index on {}",
                if index.unique { "unique " } else { "" },
                index.columns.join(", ")
            ),
            false,
        );
    }
    let removed_fks = current
        .foreign_keys
        .iter()
        .filter(|f| !plan.target.foreign_keys.iter().any(|t| same_fk(f, t)))
        .cloned()
        .collect::<Vec<_>>();
    let added_fks = plan
        .target
        .foreign_keys
        .iter()
        .filter(|f| !current.foreign_keys.iter().any(|t| same_fk(f, t)))
        .cloned()
        .collect::<Vec<_>>();
    for fk in &removed_fks {
        change(
            &mut plan,
            format!("Drop foreign key on {}", fk.column),
            false,
        );
    }
    for fk in &added_fks {
        change(
            &mut plan,
            format!(
                "Add foreign key {} -> {}.{}",
                fk.column, fk.table, fk.target
            ),
            false,
        );
    }
    if plan.changes.is_empty() {
        return Ok(plan);
    }
    if !current.unsupported.is_empty()
        || current
            .indexes
            .iter()
            .any(|i| i.columns.len() != 1 || i.columns[0].is_empty())
    {
        // Renames alone use native ALTER TABLE and preserve external SQL objects.
        if plan
            .changes
            .iter()
            .all(|c| c.description.starts_with("Rename column "))
        {
            return Ok(plan);
        }
        return Err(error(
            "migration_unsupported",
            format!(
                "table contains SQL objects outside SqlModel metadata: {}",
                current.unsupported.join(", ")
            ),
        ));
    }
    if dialect == Dialect::Sqlite {
        plan.rebuild |= !removed_fks.is_empty()
            || !added_fks.is_empty()
            || removed_indexes.iter().any(|i| i.constraint);
        if plan.rebuild {
            sqlite_rebuild::<T, DB>(&mut plan, &current)?;
        } else {
            index_statements(&mut plan, &removed_indexes, &added_indexes);
        }
    } else {
        server_alter::<T, DB>(
            &mut plan,
            &current,
            &removed_indexes,
            &added_indexes,
            &removed_fks,
            &added_fks,
        );
    }
    // Preflight queries run before native renames, against the original names.
    for (sql, _) in &mut plan.checks {
        for column in T::COLUMNS {
            if let Some(source) = column.rename_from {
                if plan.before.columns.iter().any(|c| c.name == source)
                    && !plan.before.columns.iter().any(|c| c.name == column.name)
                {
                    *sql = sql.replace(&q(column.name), &q(source));
                }
            }
        }
    }
    Ok(plan)
}
fn index_statements(plan: &mut MigrationPlan, removed: &[DbIndex], added: &[DbIndex]) {
    let q = |s: &str| plan.dialect.quote(s);
    for index in removed {
        plan.statements.push(if index.constraint {
            format!(
                "ALTER TABLE {} DROP CONSTRAINT {}",
                q(&plan.table),
                q(&index.name)
            )
        } else {
            format!("DROP INDEX {}", q(&index.name))
        });
    }
    for index in added {
        plan.statements.push(format!(
            "CREATE {}INDEX {} ON {} ({})",
            if index.unique { "UNIQUE " } else { "" },
            q(&index.name),
            q(&plan.table),
            index
                .columns
                .iter()
                .map(|c| q(c))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}
fn sqlite_rebuild<T: SqlModel, DB: SqlBackend>(
    plan: &mut MigrationPlan,
    current: &Catalog,
) -> Result<(), SqlError> {
    let q = |s: &str| Dialect::Sqlite.quote(s);
    let table = q(T::TABLE);
    let hash = super::checksum(&T::TABLE);
    let temporary = format!("_rydantic_rebuild_{}", &hash[..16]);
    let sequence = format!("_rydantic_seq_{}", &hash[..16]);
    let auto = current.columns.iter().any(|c| c.auto);
    if auto {
        plan.statements.push(format!(
            "CREATE TEMP TABLE {} AS SELECT seq FROM sqlite_sequence WHERE name = '{}'",
            q(&sequence),
            T::TABLE
        ));
    }
    plan.statements.push(T::create_table_sql::<DB>().replacen(
        &format!("CREATE TABLE IF NOT EXISTS {table}"),
        &format!("CREATE TABLE {}", q(&temporary)),
        1,
    ));
    let mut sources = Vec::new();
    for column in T::COLUMNS {
        let old = current.columns.iter().find(|c| c.name == column.name);
        let default = column.default.map(|d| d.sql(Dialect::Sqlite));
        let source = if let Some(old) = old {
            if old.kind != ddl::declaration(column, Dialect::Sqlite) {
                let allowed = match column.sql_type {
                    super::SqlType::Integer | super::SqlType::Boolean => "'integer','null'",
                    super::SqlType::Real => "'integer','real','null'",
                    super::SqlType::Text => "'integer','real','text','null'",
                    super::SqlType::Blob => "'blob','null'",
                };
                plan.checks.push((
                    format!(
                        "SELECT 1 FROM {table} WHERE typeof({}) NOT IN ({allowed}) LIMIT 1",
                        q(&old.name)
                    ),
                    format!(
                        "column {} has values incompatible with its new SQLite storage type",
                        old.name
                    ),
                ));
            }
            if !column.nullable && old.nullable && default.is_some() {
                format!(
                    "COALESCE({}, {})",
                    q(column.name),
                    default.as_deref().unwrap()
                )
            } else {
                q(column.name)
            }
        } else {
            default.unwrap_or_else(|| "NULL".into())
        };
        sources.push(source);
    }
    plan.statements.push(format!(
        "INSERT INTO {} ({}) SELECT {} FROM {table}",
        q(&temporary),
        T::COLUMNS
            .iter()
            .map(|c| q(c.name))
            .collect::<Vec<_>>()
            .join(", "),
        sources.join(", ")
    ));
    plan.statements.push(format!("DROP TABLE {table}"));
    plan.statements
        .push(format!("ALTER TABLE {} RENAME TO {table}", q(&temporary)));
    plan.statements.extend(T::create_index_sql::<DB>());
    if auto {
        plan.statements.push(format!("INSERT INTO sqlite_sequence (name,seq) SELECT '{}', COALESCE(MAX(seq),0) FROM {} WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name='{}') HAVING COUNT(*)>0",T::TABLE,q(&sequence),T::TABLE));
        plan.statements.push(format!("UPDATE sqlite_sequence SET seq = MAX(seq, COALESCE((SELECT MAX(seq) FROM {}), 0)) WHERE name = '{}'",q(&sequence),T::TABLE));
        plan.statements.push(format!("DROP TABLE {}", q(&sequence)));
    }
    Ok(())
}
fn server_alter<T: SqlModel, DB: SqlBackend>(
    plan: &mut MigrationPlan,
    current: &Catalog,
    removed_indexes: &[DbIndex],
    added_indexes: &[DbIndex],
    removed_fks: &[DbForeignKey],
    added_fks: &[DbForeignKey],
) {
    let dialect = DB::DIALECT;
    let q = |s: &str| dialect.quote(s);
    let table = q(T::TABLE);
    let mut clauses = Vec::new();
    let mut deferred_backfills = Vec::new();
    if dialect == Dialect::MySql {
        let prefix = format!("ALTER TABLE {table} ");
        clauses.extend(
            plan.statements
                .drain(..)
                .map(|s| s.strip_prefix(&prefix).expect("native rename").to_owned()),
        );
    }
    let drops = removed_fks
        .iter()
        .map(|fk| {
            format!(
                "DROP {} {}",
                if dialect == Dialect::MySql {
                    "FOREIGN KEY"
                } else {
                    "CONSTRAINT"
                },
                q(&fk.name)
            )
        })
        .collect::<Vec<_>>();
    let self_reference_index = removed_indexes.iter().any(|index| {
        removed_fks
            .iter()
            .any(|fk| fk.table == T::TABLE && index.columns == [fk.target.clone()])
    });
    if !drops.is_empty() && (dialect == Dialect::Postgres || self_reference_index) {
        // Referencing constraints must go before their supporting unique indexes.
        // MySQL also requires a separate ALTER for a self-referenced key.
        plan.statements
            .push(format!("ALTER TABLE {table} {}", drops.join(", ")));
    } else {
        clauses.extend(drops);
    }
    if dialect == Dialect::MySql {
        for index in removed_indexes {
            clauses.push(format!("DROP INDEX {}", q(&index.name)));
        }
    } else {
        index_statements(plan, removed_indexes, &[]);
    }
    for old in &current.columns {
        if !plan.target.columns.iter().any(|c| c.name == old.name) {
            clauses.push(format!("DROP COLUMN {}", q(&old.name)));
        }
    }
    for column in T::COLUMNS {
        let wanted = plan
            .target
            .columns
            .iter()
            .find(|c| c.name == column.name)
            .unwrap();
        let old = current.columns.iter().find(|c| c.name == column.name);
        let definition = || {
            format!(
                "{} {} {}{}",
                q(column.name),
                ddl::declaration(column, dialect),
                if column.nullable { "NULL" } else { "NOT NULL" },
                column
                    .default
                    .map(|d| format!(" DEFAULT {}", d.sql(dialect)))
                    .unwrap_or_default()
            )
        };
        if let Some(old) = old {
            if same_column(old, wanted) {
                continue;
            }
            if old.nullable && !column.nullable {
                if let Some(default) = column.default {
                    let statement = format!(
                        "UPDATE {table} SET {} = {} WHERE {} IS NULL",
                        q(column.name),
                        default.sql(dialect),
                        q(column.name)
                    );
                    if dialect == Dialect::MySql {
                        // Backfill uses the original name until the batched ALTER runs.
                        let original = column
                            .rename_from
                            .filter(|name| plan.before.columns.iter().any(|c| c.name == *name));
                        deferred_backfills.push(if let Some(old) = original {
                            statement.replace(&q(column.name), &q(old))
                        } else {
                            statement
                        });
                    } else {
                        plan.statements.push(statement);
                    }
                }
            }
            if dialect == Dialect::MySql {
                let renamed = column.rename_from.filter(|name| {
                    plan.before.columns.iter().any(|c| c.name == *name)
                        && !plan.before.columns.iter().any(|c| c.name == column.name)
                });
                if let Some(source) = renamed {
                    let rename = format!("RENAME COLUMN {} TO {}", q(source), q(column.name));
                    clauses.retain(|c| c != &rename);
                }
                clauses.push(format!(
                    "{} {}{}",
                    renamed
                        .map(|source| format!("CHANGE COLUMN {}", q(source)))
                        .unwrap_or_else(|| "MODIFY COLUMN".into()),
                    definition(),
                    if column.auto_increment {
                        " AUTO_INCREMENT"
                    } else {
                        ""
                    }
                ));
            } else {
                if old.kind != wanted.kind {
                    if old.default.is_some() {
                        clauses.push(format!("ALTER COLUMN {} DROP DEFAULT", q(column.name)));
                    }
                    clauses.push(format!(
                        "ALTER COLUMN {} TYPE {} USING CAST({} AS {})",
                        q(column.name),
                        ddl::declaration(column, dialect),
                        q(column.name),
                        ddl::declaration(column, dialect)
                    ));
                }
                if old.nullable != wanted.nullable {
                    clauses.push(format!(
                        "ALTER COLUMN {} {} NOT NULL",
                        q(column.name),
                        if wanted.nullable { "DROP" } else { "SET" }
                    ));
                }
                if old.default != wanted.default || old.kind != wanted.kind {
                    clauses.push(format!(
                        "ALTER COLUMN {} {}",
                        q(column.name),
                        column
                            .default
                            .map(|d| format!("SET DEFAULT {}", d.sql(dialect)))
                            .unwrap_or_else(|| "DROP DEFAULT".into())
                    ));
                }
            }
        } else {
            clauses.push(format!("ADD COLUMN {}", definition()));
        }
    }
    for fk in added_fks {
        clauses.push(format!(
            "ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {}",
            q(&fk.name),
            q(&fk.column),
            q(&fk.table),
            q(&fk.target),
            fk.on_delete
        ));
    }
    if dialect == Dialect::MySql {
        for index in added_indexes {
            clauses.push(format!(
                "ADD {}INDEX {} ({})",
                if index.unique { "UNIQUE " } else { "" },
                q(&index.name),
                index
                    .columns
                    .iter()
                    .map(|c| q(c))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if !clauses.is_empty() {
        plan.statements.extend(deferred_backfills);
        plan.statements
            .push(format!("ALTER TABLE {table} {}", clauses.join(", ")));
    }
    if dialect == Dialect::Postgres {
        index_statements(plan, &[], added_indexes);
    }
}
