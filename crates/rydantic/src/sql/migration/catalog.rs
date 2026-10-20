use super::{error, rows, text, Dialect, SqlBackend, SqlError};
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(super) struct Catalog {
    pub exists: bool,
    pub columns: Vec<DbColumn>,
    pub indexes: Vec<DbIndex>,
    pub foreign_keys: Vec<DbForeignKey>,
    pub unsupported: Vec<String>,
    pub definition: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct DbColumn {
    pub name: String,
    pub kind: String,
    pub nullable: bool,
    pub primary: bool,
    pub auto: bool,
    pub default: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct DbIndex {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub constraint: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct DbForeignKey {
    pub name: String,
    pub column: String,
    pub table: String,
    pub target: String,
    pub on_delete: String,
}
fn required<DB: SqlBackend>(row: &DB::Row, name: &str) -> Result<String, SqlError> {
    text::<DB>(row, name)?
        .ok_or_else(|| error("migration_catalog", format!("missing catalog field {name}")))
}
pub(super) async fn read<DB: SqlBackend>(
    connection: &mut DB::Connection,
    table: &str,
) -> Result<Catalog, SqlError> {
    let mut catalog = Catalog::default();
    let table_query = match DB::DIALECT {
        Dialect::Sqlite => "SELECT sql AS value FROM sqlite_schema WHERE type = 'table' AND name = {bind}",
        Dialect::Postgres => "SELECT c.relkind::text AS value FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=current_schema() AND c.relname={bind}",
        Dialect::MySql => "SELECT TABLE_TYPE AS value FROM information_schema.TABLES WHERE TABLE_SCHEMA=DATABASE() AND TABLE_NAME={bind}",
    };
    let result = rows::<DB>(connection, table_query, &[table]).await?;
    let Some(row) = result.first() else {
        return Ok(catalog);
    };
    catalog.exists = true;
    catalog.definition = required::<DB>(row, "value")?;
    if (DB::DIALECT == Dialect::Postgres && catalog.definition != "r")
        || (DB::DIALECT == Dialect::MySql && catalog.definition != "BASE TABLE")
    {
        return Err(error(
            "migration_unsupported",
            "automatic migration requires an ordinary table",
        ));
    }
    let column_query = match DB::DIALECT {
        Dialect::Sqlite => "SELECT name, upper(type) AS kind, CAST(\"notnull\" AS TEXT) AS required, CAST(pk AS TEXT) AS pk, dflt_value AS def, CAST(hidden AS TEXT) AS extra FROM pragma_table_xinfo({bind}) ORDER BY cid",
        Dialect::Postgres => "SELECT a.attname AS name, upper(format_type(a.atttypid,a.atttypmod)) AS kind, a.attnotnull::int::text AS required, (EXISTS(SELECT 1 FROM pg_index i WHERE i.indrelid=c.oid AND i.indisprimary AND a.attnum=ANY(i.indkey)))::int::text AS pk, pg_get_expr(d.adbin,d.adrelid) AS def, a.attidentity::text || a.attgenerated::text AS extra FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_attribute a ON a.attrelid=c.oid LEFT JOIN pg_attrdef d ON d.adrelid=c.oid AND d.adnum=a.attnum WHERE n.nspname=current_schema() AND c.relname={bind} AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum",
        Dialect::MySql => "SELECT COLUMN_NAME AS name, upper(COLUMN_TYPE) AS kind, CAST(IS_NULLABLE='NO' AS CHAR) AS required, CAST(COLUMN_KEY='PRI' AS CHAR) AS pk, COLUMN_DEFAULT AS def, EXTRA AS extra FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=DATABASE() AND TABLE_NAME={bind} ORDER BY ORDINAL_POSITION",
    };
    for row in rows::<DB>(connection, column_query, &[table]).await? {
        let primary = required::<DB>(&row, "pk")? != "0";
        let extra = required::<DB>(&row, "extra")?;
        let auto = match DB::DIALECT {
            Dialect::Sqlite => {
                primary && catalog.definition.to_uppercase().contains("AUTOINCREMENT")
            }
            Dialect::Postgres => extra == "d" || extra == "a",
            Dialect::MySql => extra.contains("auto_increment"),
        };
        if (DB::DIALECT == Dialect::Sqlite && extra != "0")
            || (DB::DIALECT == Dialect::Postgres && !matches!(extra.as_str(), "" | "a" | "d"))
            || (DB::DIALECT == Dialect::MySql
                && extra.contains("GENERATED")
                && !extra.contains("DEFAULT_GENERATED"))
        {
            catalog.unsupported.push("generated columns".into());
        }
        let kind = normalize_type(&required::<DB>(&row, "kind")?, DB::DIALECT);
        let default = if auto {
            None
        } else {
            text::<DB>(&row, "def")?.map(|s| {
                if DB::DIALECT == Dialect::MySql
                    && extra.contains("DEFAULT_GENERATED")
                    && mysql_text_default(&s).is_none()
                {
                    format!("expression:{s}")
                } else {
                    normalize_default(&s, &kind, DB::DIALECT)
                }
            })
        };
        catalog.columns.push(DbColumn {
            name: required::<DB>(&row, "name")?,
            kind,
            nullable: !primary && required::<DB>(&row, "required")? == "0",
            primary,
            auto,
            default,
        });
    }
    if catalog.columns.iter().filter(|c| c.primary).count() != 1 {
        catalog
            .unsupported
            .push("composite or absent primary key".into());
    }
    match DB::DIALECT {
        Dialect::Sqlite => sqlite_objects::<DB>(connection, table, &mut catalog).await?,
        Dialect::Postgres => postgres_objects::<DB>(connection, table, &mut catalog).await?,
        Dialect::MySql => mysql_objects::<DB>(connection, table, &mut catalog).await?,
    }
    catalog.indexes.sort_by(|a, b| a.name.cmp(&b.name));
    catalog.foreign_keys.sort_by(|a, b| a.name.cmp(&b.name));
    catalog.unsupported.sort();
    Ok(catalog)
}
async fn sqlite_objects<DB: SqlBackend>(
    connection: &mut DB::Connection,
    table: &str,
    catalog: &mut Catalog,
) -> Result<(), SqlError> {
    for row in rows::<DB>(connection, "SELECT name, CAST(\"unique\" AS TEXT) AS uq, origin, CAST(partial AS TEXT) AS partial FROM pragma_index_list({bind})", &[table]).await? {
        let name = required::<DB>(&row, "name")?;
        let mut columns = Vec::new();
        for row in rows::<DB>(connection, "SELECT name, coll, CAST(desc AS TEXT) AS descending FROM pragma_index_xinfo({bind}) WHERE key=1 ORDER BY seqno", &[&name]).await? {
            columns.push(text::<DB>(&row, "name")?.unwrap_or_default());
            if !required::<DB>(&row, "coll")?.eq_ignore_ascii_case("BINARY") || required::<DB>(&row, "descending")? != "0" {
                catalog.unsupported.push("custom index collation or ordering".into());
            }
        }
        // Primary-key indexes are excluded from index reconciliation, but their
        // collation and ordering must still be checked before a table rebuild.
        if required::<DB>(&row, "origin")? == "pk" { continue; }
        if required::<DB>(&row, "partial")? != "0" { catalog.unsupported.push("partial indexes".into()); }
        catalog.indexes.push(DbIndex {name, columns, unique: required::<DB>(&row,"uq")? == "1", constraint: required::<DB>(&row,"origin")? == "u"});
    }
    for row in rows::<DB>(connection, "SELECT CAST(id AS TEXT) AS name, CAST(seq AS TEXT) AS seq, \"from\" AS source, \"table\" AS dest, \"to\" AS target, on_delete, on_update, match FROM pragma_foreign_key_list({bind}) ORDER BY id, seq", &[table]).await? {
        if required::<DB>(&row,"seq")? != "0" || required::<DB>(&row,"on_update")? != "NO ACTION" || required::<DB>(&row,"match")? != "NONE" {
            catalog.unsupported.push("composite or custom foreign keys".into());
        }
        catalog.foreign_keys.push(DbForeignKey { name: required::<DB>(&row,"name")?, column: required::<DB>(&row,"source")?, table: required::<DB>(&row,"dest")?, target: required::<DB>(&row,"target")?, on_delete: required::<DB>(&row,"on_delete")? });
    }
    // A rebuild must never silently discard application-managed SQL objects.
    for row in rows::<DB>(connection, "SELECT type AS value FROM sqlite_schema WHERE (type='trigger' AND tbl_name={bind}) OR type='view' ORDER BY name", &[table]).await? {
        catalog.unsupported.push(required::<DB>(&row,"value")?);
    }
    let definition = unquoted_sql(&catalog.definition);
    if regex::Regex::new(r"(?i)\bCHECK\s*\(|\bCOLLATE\b|\bWITHOUT\s+ROWID\b|\bSTRICT\b|\bDEFERRABLE\b|\bON\s+CONFLICT\b").expect("SQLite custom rules").is_match(&definition)
    {
        catalog
            .unsupported
            .push("custom table constraints or storage options".into());
    }
    Ok(())
}
async fn postgres_objects<DB: SqlBackend>(
    connection: &mut DB::Connection,
    table: &str,
    catalog: &mut Catalog,
) -> Result<(), SqlError> {
    let sql = "SELECT x.relname AS name, i.indisunique::int::text AS uq, (co.oid IS NOT NULL)::int::text AS con, a.attname AS col, (i.indpred IS NOT NULL OR i.indexprs IS NOT NULL OR i.indnkeyatts<>i.indnatts)::int::text AS custom FROM pg_class t JOIN pg_namespace n ON n.oid=t.relnamespace JOIN pg_index i ON i.indrelid=t.oid JOIN pg_class x ON x.oid=i.indexrelid LEFT JOIN pg_constraint co ON co.conindid=i.indexrelid AND co.contype='u' CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY k(num,seq) LEFT JOIN pg_attribute a ON a.attrelid=t.oid AND a.attnum=k.num WHERE n.nspname=current_schema() AND t.relname={bind} AND NOT i.indisprimary ORDER BY x.relname,k.seq";
    for row in rows::<DB>(connection, sql, &[table]).await? {
        let name = required::<DB>(&row, "name")?;
        let column = text::<DB>(&row, "col")?.unwrap_or_default();
        if required::<DB>(&row, "custom")? == "1" {
            catalog
                .unsupported
                .push("expression, partial or covering indexes".into());
        }
        if let Some(index) = catalog.indexes.iter_mut().find(|i| i.name == name) {
            index.columns.push(column);
        } else {
            catalog.indexes.push(DbIndex {
                name,
                columns: vec![column],
                unique: required::<DB>(&row, "uq")? == "1",
                constraint: required::<DB>(&row, "con")? == "1",
            });
        }
    }
    let sql = "SELECT co.conname AS name, a.attname AS source, f.relname AS dest, b.attname AS target, co.confdeltype::text AS action, (cardinality(co.conkey)<>1 OR co.condeferrable OR co.confupdtype<>'a' OR fn.nspname<>current_schema())::int::text AS custom FROM pg_constraint co JOIN pg_class t ON t.oid=co.conrelid JOIN pg_namespace n ON n.oid=t.relnamespace JOIN pg_class f ON f.oid=co.confrelid JOIN pg_namespace fn ON fn.oid=f.relnamespace JOIN pg_attribute a ON a.attrelid=t.oid AND a.attnum=co.conkey[1] JOIN pg_attribute b ON b.attrelid=f.oid AND b.attnum=co.confkey[1] WHERE n.nspname=current_schema() AND t.relname={bind} AND co.contype='f' ORDER BY co.conname";
    for row in rows::<DB>(connection, sql, &[table]).await? {
        if required::<DB>(&row, "custom")? == "1" {
            catalog
                .unsupported
                .push("composite, cross-schema or custom foreign keys".into());
        }
        let action = match required::<DB>(&row, "action")?.as_str() {
            "c" => "CASCADE",
            "n" => "SET NULL",
            "r" => "RESTRICT",
            "d" => "SET DEFAULT",
            _ => "NO ACTION",
        };
        catalog.foreign_keys.push(DbForeignKey {
            name: required::<DB>(&row, "name")?,
            column: required::<DB>(&row, "source")?,
            table: required::<DB>(&row, "dest")?,
            target: required::<DB>(&row, "target")?,
            on_delete: action.into(),
        });
    }
    Ok(())
}
async fn mysql_objects<DB: SqlBackend>(
    connection: &mut DB::Connection,
    table: &str,
    catalog: &mut Catalog,
) -> Result<(), SqlError> {
    for row in rows::<DB>(connection, "SELECT INDEX_NAME AS name, COLUMN_NAME AS col, CAST(NON_UNIQUE=0 AS CHAR) AS uq, CAST(SUB_PART IS NOT NULL OR EXPRESSION IS NOT NULL AS CHAR) AS custom FROM information_schema.STATISTICS WHERE TABLE_SCHEMA=DATABASE() AND TABLE_NAME={bind} AND INDEX_NAME<>'PRIMARY' ORDER BY INDEX_NAME,SEQ_IN_INDEX", &[table]).await? {
        let name = required::<DB>(&row,"name")?;
        let column = text::<DB>(&row,"col")?.unwrap_or_default();
        if required::<DB>(&row,"custom")? == "1" { catalog.unsupported.push("prefix or expression indexes".into()); }
        if let Some(index) = catalog.indexes.iter_mut().find(|i|i.name==name) { index.columns.push(column); }
        else { catalog.indexes.push(DbIndex {name,columns:vec![column],unique:required::<DB>(&row,"uq")?=="1",constraint:false}); }
    }
    let sql = "SELECT k.CONSTRAINT_NAME AS name,k.COLUMN_NAME AS source,k.REFERENCED_TABLE_NAME AS dest,k.REFERENCED_COLUMN_NAME AS target,CASE r.DELETE_RULE WHEN 'NO ACTION' THEN 'RESTRICT' ELSE r.DELETE_RULE END AS action,CAST(k.ORDINAL_POSITION<>1 OR r.UPDATE_RULE<>'NO ACTION' AND r.UPDATE_RULE<>'RESTRICT' OR k.REFERENCED_TABLE_SCHEMA<>DATABASE() AS CHAR) AS custom FROM information_schema.KEY_COLUMN_USAGE k JOIN information_schema.REFERENTIAL_CONSTRAINTS r ON r.CONSTRAINT_SCHEMA=k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME=k.CONSTRAINT_NAME AND r.TABLE_NAME=k.TABLE_NAME WHERE k.TABLE_SCHEMA=DATABASE() AND k.TABLE_NAME={bind} AND k.REFERENCED_TABLE_NAME IS NOT NULL ORDER BY k.CONSTRAINT_NAME,k.ORDINAL_POSITION";
    for row in rows::<DB>(connection, sql, &[table]).await? {
        if required::<DB>(&row, "custom")? == "1" {
            catalog
                .unsupported
                .push("composite, cross-schema or custom foreign keys".into());
        }
        catalog.foreign_keys.push(DbForeignKey {
            name: required::<DB>(&row, "name")?,
            column: required::<DB>(&row, "source")?,
            table: required::<DB>(&row, "dest")?,
            target: required::<DB>(&row, "target")?,
            on_delete: required::<DB>(&row, "action")?,
        });
    }
    Ok(())
}
pub(super) fn normalize_type(value: &str, dialect: Dialect) -> String {
    let value = value.to_uppercase();
    match (dialect, value.as_str()) {
        (Dialect::MySql, "BOOLEAN" | "TINYINT(1)") => "TINYINT".into(),
        (Dialect::Postgres, "INT8") => "BIGINT".into(),
        _ => value,
    }
}
fn mysql_text_default(value: &str) -> Option<String> {
    let literal = regex::Regex::new(
        r"(?i)^\(*convert\((?:x'([a-f0-9]*)'|0x([a-f0-9]*))\s+using\s+utf8mb4\)\)*$",
    )
    .expect("MySQL constant text default");
    let capture = literal.captures(value.trim())?;
    let hex = capture.get(1).or_else(|| capture.get(2))?.as_str();
    let bytes: Option<Vec<u8>> = (0..hex.len())
        .step_by(2)
        .map(|i| {
            hex.get(i..i + 2)
                .and_then(|v| u8::from_str_radix(v, 16).ok())
        })
        .collect();
    String::from_utf8(bytes?).ok()
}
fn unquoted_sql(value: &str) -> String {
    let mut result = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(c, '\'' | '"' | '`' | '[') {
            let end = if c == '[' { ']' } else { c };
            while let Some(c) = chars.next() {
                if c == end {
                    if chars.peek() == Some(&end) {
                        chars.next();
                    } else {
                        break;
                    }
                }
            }
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}
pub(super) fn normalize_default(value: &str, kind: &str, dialect: Dialect) -> String {
    let value = value.trim();
    if dialect != Dialect::MySql && value.eq_ignore_ascii_case("NULL") {
        return "null".into();
    }
    if kind == "BOOLEAN" && dialect == Dialect::Postgres {
        return format!("bool:{value}");
    }
    if matches!(
        kind,
        "INTEGER" | "BIGINT" | "REAL" | "DOUBLE" | "DOUBLE PRECISION" | "TINYINT"
    ) {
        let numeric = regex::Regex::new(
            r"^\(*'?[-+]?[0-9]+(?:\.[0-9]+)?(?:[eE][-+]?[0-9]+)?'?(?:::[a-zA-Z ]+)?\)*$",
        )
        .expect("numeric literal pattern");
        let value = if numeric.is_match(value) {
            value
                .split("::")
                .next()
                .unwrap_or(value)
                .trim_matches(['(', ')', '\''])
        } else {
            ""
        };
        if let Ok(number) = value.parse::<i64>() {
            return format!("number:{number}");
        }
        if let Ok(number) = value.parse::<f64>() {
            return format!("number:{number}");
        }
    }
    if dialect == Dialect::MySql {
        if let Some(text) = mysql_text_default(value) {
            return format!("text:{text}");
        }
    }
    let raw = value
        .strip_prefix("E'")
        .or_else(|| value.strip_prefix("e'"))
        .or_else(|| value.strip_prefix('\''));
    if let Some(raw) = raw {
        let simple_cast=regex::Regex::new(r"^(?:::(?:text|character varying(?:\([0-9]+\))?|character(?:\([0-9]+\))?|bpchar|varchar|name))?$").expect("text literal pattern");
        let mut text = String::new();
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    text.push('\'');
                } else {
                    let tail: String = chars.collect();
                    if tail.is_empty()
                        || dialect == Dialect::Postgres && simple_cast.is_match(tail.trim())
                    {
                        return format!("text:{text}");
                    }
                    return format!("expression:{value}");
                }
            } else if c == '\\' && (dialect == Dialect::MySql || value.starts_with("E'")) {
                if let Some(c) = chars.next() {
                    text.push(match c {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        other => other,
                    });
                }
            } else {
                text.push(c);
            }
        }
    }
    if dialect == Dialect::MySql
        && !matches!(kind, "INTEGER" | "BIGINT" | "REAL" | "DOUBLE" | "TINYINT")
        && !value.starts_with('(')
    {
        return format!("text:{value}");
    }
    format!("expression:{value}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_preserve_literals_and_distinguish_expressions() {
        assert_eq!(
            normalize_default("'-7'::bigint", "BIGINT", Dialect::Postgres),
            "number:-7"
        );
        assert_eq!(
            normalize_default("'-1.5'::numeric", "DOUBLE PRECISION", Dialect::Postgres),
            "number:-1.5"
        );
        assert!(
            normalize_default("'1'::bigint + 2", "BIGINT", Dialect::Postgres)
                .starts_with("expression:")
        );
        assert!(
            normalize_default("'a'::text || 'b'::text", "TEXT", Dialect::Postgres)
                .starts_with("expression:")
        );
        assert_eq!(
            normalize_default("NULL", "LONGTEXT", Dialect::MySql),
            "text:NULL"
        );
        assert_eq!(
            normalize_default(
                "convert(0x2020612020 using utf8mb4)",
                "LONGTEXT",
                Dialect::MySql
            ),
            "text:  a  "
        );
        assert_eq!(
            normalize_default("'O''Reilly'::text", "TEXT", Dialect::Postgres),
            "text:O'Reilly"
        );
    }
}
