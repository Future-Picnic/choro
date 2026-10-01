//! Synchronous relational database backends for Choro's database browser.
//! Calls in this module block and must be dispatched to a background executor.

use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, MutexGuard,
};

use anyhow::{anyhow, bail, Context as _, Result};
use mysql::prelude::Queryable as _;
use postgres::config::SslMode;
use postgres_native_tls::MakeTlsConnector;
use rusqlite::types::{Value as SqliteValue, ValueRef};
use serde_json::{Map, Number, Value};

use crate::{DbObject, DbObjectKind, DbProvider, TableColumn, TableFilter, TablePage, TableRow};

const CONNECT_TIMEOUT_SECS: u64 = 7;

#[derive(Clone)]
pub struct SqlHandle {
    provider: DbProvider,
    uri: String,
    read_only: Arc<AtomicBool>,
    postgres: Arc<Mutex<Option<postgres::Client>>>,
    mysql: Arc<Mutex<Option<mysql::Pool>>>,
    remote: Option<Arc<crate::db_http::HttpSqlHandle>>,
}

/// Keeps the session exclusively checked out for an entire operation, including
/// its transaction. Cloned browser handles share this same session.
struct PostgresSession<'a>(MutexGuard<'a, Option<postgres::Client>>);

impl std::ops::Deref for PostgresSession<'_> {
    type Target = postgres::Client;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("checked-out PostgreSQL session")
    }
}

impl std::ops::DerefMut for PostgresSession<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().expect("checked-out PostgreSQL session")
    }
}

impl SqlHandle {
    pub fn new(provider: DbProvider, uri: impl Into<String>) -> Result<Self> {
        Self::new_with_access(provider, uri, false)
    }

    pub(crate) fn new_with_access(
        provider: DbProvider,
        uri: impl Into<String>,
        read_only: bool,
    ) -> Result<Self> {
        let uri = uri.into();
        match provider {
            DbProvider::PostgreSql | DbProvider::Supabase => {
                let parsed =
                    url::Url::parse(&uri).context("invalid PostgreSQL connection string")?;
                if !matches!(parsed.scheme(), "postgres" | "postgresql") {
                    bail!("expected a postgres:// or postgresql:// connection string");
                }
                if provider == DbProvider::Supabase && parsed.port() == Some(6543) {
                    bail!(
                        "Supabase transaction-pooler URLs (port 6543) do not support the prepared statements Choro uses; copy the Session pooler URL on port 5432 from Supabase → Connect"
                    );
                }
            }
            DbProvider::MySql | DbProvider::MariaDb => {
                let parsed = url::Url::parse(&uri).context("invalid MySQL connection string")?;
                if parsed.scheme() != "mysql" {
                    bail!("expected a mysql:// connection string");
                }
            }
            DbProvider::SQLite => {
                let path = sqlite_path(&uri)?;
                if path.as_os_str().is_empty() {
                    bail!("SQLite database path is empty");
                }
            }
            DbProvider::MongoDb => bail!("MongoDB is not a relational backend"),
            DbProvider::Turso | DbProvider::ClickHouse => {}
        }
        let remote = if provider.is_http() {
            Some(Arc::new(crate::db_http::HttpSqlHandle::new(
                provider, &uri,
            )?))
        } else {
            None
        };
        Ok(Self {
            provider,
            uri,
            read_only: Arc::new(AtomicBool::new(read_only)),
            postgres: Arc::new(Mutex::new(None)),
            mysql: Arc::new(Mutex::new(None)),
            remote,
        })
    }

    pub fn provider(&self) -> DbProvider {
        self.provider
    }

    pub(crate) fn shares_connection(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.read_only, &other.read_only)
    }

    pub fn is_read_only(&self) -> bool {
        !self.provider.capabilities().row_editing || self.read_only.load(Ordering::Relaxed)
    }

    pub fn set_read_only(&self, read_only: bool) {
        self.read_only.store(read_only, Ordering::Relaxed);
    }

    /// Validate authentication and transport without requiring catalog access.
    pub fn ping(&self) -> Result<()> {
        if let Some(remote) = &self.remote {
            return remote.ping();
        } else if self.provider.is_postgres() {
            self.postgres_client()?.simple_query("SELECT 1")?;
        } else if self.provider.is_mysql() {
            self.mysql_conn()?.query_drop("SELECT 1")?;
        } else {
            self.sqlite_conn()?.query_row("SELECT 1", [], |_| Ok(()))?;
        }
        Ok(())
    }

    pub fn list_namespaces(&self) -> Result<Vec<String>> {
        if let Some(remote) = &self.remote {
            return remote.list_namespaces();
        }
        if self.provider.is_postgres() {
            let mut client = self.postgres_client()?;
            let rows = client.query(
                "SELECT schema_name FROM information_schema.schemata
                 WHERE schema_name NOT IN ('pg_catalog', 'information_schema')
                   AND schema_name NOT LIKE 'pg_toast%'
                   AND schema_name NOT LIKE 'pg_temp_%'
                 ORDER BY schema_name",
                &[],
            )?;
            return Ok(rows.into_iter().map(|row| row.get(0)).collect());
        }
        if self.provider.is_mysql() {
            let mut conn = self.mysql_conn()?;
            let names: Vec<String> = conn.query(
                "SELECT schema_name FROM information_schema.schemata
                 WHERE schema_name NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
                 ORDER BY schema_name",
            )?;
            return Ok(names);
        }

        let conn = self.sqlite_conn()?;
        let mut statement = conn.prepare("PRAGMA database_list")?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(names)
    }

    pub fn list_objects(&self, namespace: &str) -> Result<Vec<DbObject>> {
        if let Some(remote) = &self.remote {
            return remote.list_objects(namespace);
        }
        if self.provider.is_postgres() {
            let mut client = self.postgres_client()?;
            let rows = client.query(
                "SELECT table_name, table_type FROM information_schema.tables
                 WHERE table_schema = $1
                 ORDER BY table_name",
                &[&namespace],
            )?;
            return Ok(rows
                .into_iter()
                .map(|row| DbObject {
                    name: row.get(0),
                    kind: object_kind(&row.get::<_, String>(1)),
                })
                .collect());
        }
        if self.provider.is_mysql() {
            let mut conn = self.mysql_conn()?;
            let rows: Vec<(String, String)> = conn.exec(
                "SELECT table_name, table_type FROM information_schema.tables
                 WHERE table_schema = ? ORDER BY table_name",
                (namespace,),
            )?;
            return Ok(rows
                .into_iter()
                .map(|(name, kind)| DbObject {
                    name,
                    kind: object_kind(&kind),
                })
                .collect());
        }

        let conn = self.sqlite_conn()?;
        let sql = format!(
            "SELECT name, type FROM {}.sqlite_master
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
            quote_sqlite_ident(namespace)
        );
        let mut statement = conn.prepare(&sql)?;
        let objects = statement
            .query_map([], |row| {
                let name: String = row.get(0)?;
                let kind: String = row.get(1)?;
                Ok(DbObject {
                    name,
                    kind: object_kind(&kind),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(objects)
    }

    pub fn fetch_rows(
        &self,
        namespace: &str,
        table: &str,
        filters: &[TableFilter],
        offset: u64,
        limit: u64,
    ) -> Result<TablePage> {
        if let Some(remote) = &self.remote {
            return remote.fetch_rows(namespace, table, filters, offset, limit);
        }
        if self.provider.is_postgres() {
            return self.postgres_fetch(namespace, table, filters, offset, limit);
        }
        if self.provider.is_mysql() {
            return self.mysql_fetch(namespace, table, filters, offset, limit);
        }
        self.sqlite_fetch(namespace, table, filters, offset, limit)
    }

    pub fn update_row(
        &self,
        namespace: &str,
        table: &str,
        original_json: &str,
        edited_json: &str,
    ) -> Result<()> {
        if self.is_read_only() {
            bail!("connection is read-only");
        }
        let original = parse_row_object(original_json)?;
        let edited = parse_row_object(edited_json)?;
        if self.provider.is_postgres() {
            return self.postgres_update(namespace, table, original, edited);
        }
        if self.provider.is_mysql() {
            return self.mysql_update(namespace, table, original, edited);
        }
        self.sqlite_update(namespace, table, original, edited)
    }

    fn postgres_client(&self) -> Result<PostgresSession<'_>> {
        let mut session = self
            .postgres
            .lock()
            .map_err(|_| anyhow!("PostgreSQL session lock failed"))?;
        // Reconnect only before a new operation. Never replay an operation that
        // may already have committed when its response was lost.
        if session.as_ref().is_none_or(postgres::Client::is_closed) {
            *session = Some(self.open_postgres_client()?);
        }
        Ok(PostgresSession(session))
    }

    fn open_postgres_client(&self) -> Result<postgres::Client> {
        let mut config: postgres::Config = self
            .uri
            .parse()
            .map_err(|error| anyhow!("invalid PostgreSQL connection string: {error}"))?;
        config.connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS));
        if config.get_application_name().is_none() {
            config.application_name("Choro");
        }
        if self.provider == DbProvider::Supabase {
            config.ssl_mode(SslMode::Require);
        }
        // Keep normal certificate and hostname verification for every provider,
        // including Supabase. A failed trust check must fail closed rather than
        // silently exposing production credentials to a man-in-the-middle.
        let tls = native_tls::TlsConnector::builder()
            .build()
            .context("failed to initialize TLS")?;
        config
            .connect(MakeTlsConnector::new(tls))
            .map_err(|error| {
                let error = redact(&error);
                if self.provider == DbProvider::Supabase {
                    anyhow!(
                        "Supabase connection failed: {error}. Copy the Session pooler URL (port 5432) from the project's Connect dialog if the direct IPv6 endpoint is unavailable"
                    )
                } else {
                    anyhow!("PostgreSQL connection failed: {error}")
                }
            })
    }

    fn mysql_conn(&self) -> Result<mysql::PooledConn> {
        let pool = {
            let mut cached = self
                .mysql
                .lock()
                .map_err(|_| anyhow!("MySQL pool lock failed"))?;
            if cached.is_none() {
                let opts = mysql::Opts::from_url(&self.uri).map_err(|error| {
                    anyhow!("invalid MySQL connection string: {}", redact(&error))
                })?;
                let read_timeout = opts
                    .get_read_timeout()
                    .copied()
                    .unwrap_or(std::time::Duration::from_secs(30));
                let write_timeout = opts
                    .get_write_timeout()
                    .copied()
                    .unwrap_or(std::time::Duration::from_secs(30));
                let opts = mysql::OptsBuilder::from_opts(opts)
                    .tcp_connect_timeout(Some(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS)))
                    .read_timeout(Some(read_timeout))
                    .write_timeout(Some(write_timeout));
                *cached = Some(mysql::Pool::new(opts).map_err(|error| {
                    anyhow!("failed to create MySQL connection pool: {}", redact(&error))
                })?);
            }
            cached.as_ref().expect("initialized MySQL pool").clone()
        };
        pool.try_get_conn(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .map_err(|error| anyhow!("MySQL connection failed: {}", redact(&error)))
    }

    fn sqlite_conn(&self) -> Result<rusqlite::Connection> {
        let path = sqlite_path(&self.uri)?;
        rusqlite::Connection::open_with_flags(
            &path,
            (if self.is_read_only() {
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            } else {
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
            }) | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("failed to open SQLite database at {}", path.display()))
    }

    fn postgres_columns(
        &self,
        client: &mut postgres::Client,
        namespace: &str,
        table: &str,
    ) -> Result<(Vec<TableColumn>, DbObjectKind)> {
        let rows = client.query(
            "SELECT c.column_name, c.data_type, c.is_nullable = 'YES',
                    COALESCE(c.is_generated = 'NEVER', true)
                      AND COALESCE(c.is_identity = 'NO', true),
                    EXISTS (
                      SELECT 1
                      FROM information_schema.table_constraints tc
                      JOIN information_schema.key_column_usage kcu
                        ON tc.constraint_name = kcu.constraint_name
                       AND tc.table_schema = kcu.table_schema
                       AND tc.table_name = kcu.table_name
                      WHERE tc.constraint_type = 'PRIMARY KEY'
                        AND tc.table_schema = c.table_schema
                        AND tc.table_name = c.table_name
                        AND kcu.column_name = c.column_name
                    ) AS primary_key
             FROM information_schema.columns c
             WHERE c.table_schema = $1 AND c.table_name = $2
             ORDER BY c.ordinal_position",
            &[&namespace, &table],
        )?;
        if rows.is_empty() {
            bail!("table or view no longer exists");
        }
        let columns = rows
            .into_iter()
            .map(|row| TableColumn {
                name: row.get(0),
                data_type: row.get(1),
                nullable: row.get(2),
                editable: row.get(3),
                primary_key: row.get(4),
            })
            .collect();
        // Use the checked-out session rather than recursively checking it out.
        let kind = client.query_one(
            "SELECT table_type FROM information_schema.tables WHERE table_schema = $1 AND table_name = $2",
            &[&namespace, &table],
        )?;
        let object_kind = object_kind(&kind.get::<_, String>(0));
        Ok((columns, object_kind))
    }

    fn postgres_fetch(
        &self,
        namespace: &str,
        table: &str,
        filters: &[TableFilter],
        offset: u64,
        limit: u64,
    ) -> Result<TablePage> {
        let mut client = self.postgres_client()?;
        let (columns, object_kind) = self.postgres_columns(&mut client, namespace, table)?;
        validate_filters(&columns, filters)?;
        let qualified = pg_qualified(namespace, table);
        let mut sql = format!("SELECT row_to_json(t)::text FROM {qualified} AS t");
        let mut values = Vec::new();
        if !filters.is_empty() {
            sql.push_str(" WHERE ");
            for (index, filter) in filters.iter().enumerate() {
                if index > 0 {
                    sql.push_str(" AND ");
                }
                sql.push_str(&format!(
                    "CAST(t.{} AS TEXT) ILIKE ${}",
                    quote_pg_ident(&filter.column),
                    index + 1
                ));
                values.push(format!("%{}%", filter.value));
            }
        }
        append_order_by(&mut sql, &columns, quote_pg_ident);
        let limit_parameter = values.len() + 1;
        sql.push_str(&format!(
            " LIMIT ${limit_parameter} OFFSET ${}",
            limit_parameter + 1
        ));
        let requested = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
        let offset = i64::try_from(offset).unwrap_or(i64::MAX);
        let mut params: Vec<&(dyn postgres::types::ToSql + Sync)> = values
            .iter()
            .map(|value| value as &(dyn postgres::types::ToSql + Sync))
            .collect();
        params.push(&requested);
        params.push(&offset);
        let mut rows = client
            .query(&sql, &params)
            .context("PostgreSQL table query failed")?
            .into_iter()
            .map(|row| TableRow { json: row.get(0) })
            .collect::<Vec<_>>();
        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        Ok(TablePage {
            columns,
            rows,
            has_more,
            editable: !self.is_read_only() && object_kind == DbObjectKind::Table,
        })
    }

    fn mysql_columns(
        &self,
        conn: &mut mysql::PooledConn,
        namespace: &str,
        table: &str,
    ) -> Result<(Vec<TableColumn>, DbObjectKind)> {
        let rows: Vec<(String, String, String, String, String)> = conn.exec(
            "SELECT column_name, data_type, is_nullable, extra, column_key
             FROM information_schema.columns
             WHERE table_schema = ? AND table_name = ?
             ORDER BY ordinal_position",
            (namespace, table),
        )?;
        if rows.is_empty() {
            bail!("table or view no longer exists");
        }
        let columns = rows
            .into_iter()
            .map(|(name, data_type, nullable, extra, key)| TableColumn {
                name,
                data_type,
                nullable: nullable == "YES",
                editable: !extra.to_ascii_lowercase().contains("generated"),
                primary_key: key == "PRI",
            })
            .collect();
        let kind: Option<String> = conn.exec_first(
            "SELECT table_type FROM information_schema.tables WHERE table_schema = ? AND table_name = ?",
            (namespace, table),
        )?;
        let object_kind =
            object_kind(&kind.ok_or_else(|| anyhow!("table or view no longer exists"))?);
        Ok((columns, object_kind))
    }

    fn mysql_fetch(
        &self,
        namespace: &str,
        table: &str,
        filters: &[TableFilter],
        offset: u64,
        limit: u64,
    ) -> Result<TablePage> {
        let mut conn = self.mysql_conn()?;
        let (columns, object_kind) = self.mysql_columns(&mut conn, namespace, table)?;
        validate_filters(&columns, filters)?;
        let selected = columns
            .iter()
            .map(|column| quote_mysql_ident(&column.name))
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = format!(
            "SELECT {selected} FROM {}.{}",
            quote_mysql_ident(namespace),
            quote_mysql_ident(table)
        );
        let mut params = Vec::new();
        if !filters.is_empty() {
            sql.push_str(" WHERE ");
            for (index, filter) in filters.iter().enumerate() {
                if index > 0 {
                    sql.push_str(" AND ");
                }
                sql.push_str(&format!(
                    "CAST({} AS CHAR) LIKE ?",
                    quote_mysql_ident(&filter.column)
                ));
                params.push(mysql::Value::Bytes(
                    format!("%{}%", filter.value).into_bytes(),
                ));
            }
        }
        append_order_by(&mut sql, &columns, quote_mysql_ident);
        sql.push_str(" LIMIT ? OFFSET ?");
        params.push(mysql::Value::UInt(limit.saturating_add(1)));
        params.push(mysql::Value::UInt(offset));
        let raw_rows: Vec<mysql::Row> = conn.exec(sql, mysql::Params::Positional(params))?;
        let mut rows = raw_rows
            .into_iter()
            .map(|row| {
                Ok(TableRow {
                    json: mysql_row_json(&columns, row)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        Ok(TablePage {
            columns,
            rows,
            has_more,
            editable: !self.is_read_only() && object_kind == DbObjectKind::Table,
        })
    }

    fn sqlite_columns(
        &self,
        conn: &rusqlite::Connection,
        namespace: &str,
        table: &str,
    ) -> Result<(Vec<TableColumn>, DbObjectKind)> {
        let sql = format!(
            "PRAGMA {}.table_xinfo({})",
            quote_sqlite_ident(namespace),
            quote_sqlite_string(table)
        );
        let mut statement = conn.prepare(&sql)?;
        let columns = statement
            .query_map([], |row| {
                let hidden: i64 = row.get(6)?;
                Ok(TableColumn {
                    name: row.get(1)?,
                    data_type: row.get::<_, String>(2)?,
                    nullable: row.get::<_, i64>(3)? == 0,
                    primary_key: row.get::<_, i64>(5)? > 0,
                    editable: hidden == 0,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if columns.is_empty() {
            bail!("table or view no longer exists");
        }
        let object_kind = self
            .list_objects(namespace)?
            .into_iter()
            .find(|object| object.name == table)
            .map(|object| object.kind)
            .unwrap_or(DbObjectKind::Table);
        Ok((columns, object_kind))
    }

    fn sqlite_fetch(
        &self,
        namespace: &str,
        table: &str,
        filters: &[TableFilter],
        offset: u64,
        limit: u64,
    ) -> Result<TablePage> {
        let conn = self.sqlite_conn()?;
        let (columns, object_kind) = self.sqlite_columns(&conn, namespace, table)?;
        validate_filters(&columns, filters)?;
        let selected = columns
            .iter()
            .map(|column| quote_sqlite_ident(&column.name))
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = format!(
            "SELECT {selected} FROM {}.{}",
            quote_sqlite_ident(namespace),
            quote_sqlite_ident(table)
        );
        let mut params = Vec::new();
        if !filters.is_empty() {
            sql.push_str(" WHERE ");
            for (index, filter) in filters.iter().enumerate() {
                if index > 0 {
                    sql.push_str(" AND ");
                }
                sql.push_str(&format!(
                    "CAST({} AS TEXT) LIKE ? COLLATE NOCASE",
                    quote_sqlite_ident(&filter.column)
                ));
                params.push(SqliteValue::Text(format!("%{}%", filter.value)));
            }
        }
        append_order_by(&mut sql, &columns, quote_sqlite_ident);
        sql.push_str(" LIMIT ? OFFSET ?");
        params.push(SqliteValue::Integer(
            i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX),
        ));
        params.push(SqliteValue::Integer(
            i64::try_from(offset).unwrap_or(i64::MAX),
        ));
        let mut statement = conn.prepare(&sql)?;
        let mut query = statement.query(rusqlite::params_from_iter(params))?;
        let mut rows = Vec::new();
        while let Some(row) = query.next()? {
            rows.push(TableRow {
                json: sqlite_row_json(&columns, row)?,
            });
        }
        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        Ok(TablePage {
            columns,
            rows,
            has_more,
            editable: !self.is_read_only() && object_kind == DbObjectKind::Table,
        })
    }

    fn postgres_update(
        &self,
        namespace: &str,
        table: &str,
        original: Map<String, Value>,
        edited: Map<String, Value>,
    ) -> Result<()> {
        let mut client = self.postgres_client()?;
        if self.is_read_only() {
            bail!("connection is read-only");
        }
        let (columns, kind) = self.postgres_columns(&mut client, namespace, table)?;
        let changed = validate_edit(&columns, kind, &original, &edited)?;
        if changed.is_empty() {
            return Ok(());
        }
        let keys = primary_keys(&columns)?;
        let qualified = pg_qualified(namespace, table);
        let key_match = keys
            .iter()
            .map(|key| {
                let key = quote_pg_ident(key);
                format!("t.{key} IS NOT DISTINCT FROM o.{key}")
            })
            .collect::<Vec<_>>()
            .join(" AND ");
        let original_value = Value::Object(original.clone());
        let edited_value = Value::Object(edited);
        let mut transaction = client.transaction()?;
        let lock_sql = format!(
            "WITH o AS (SELECT * FROM jsonb_populate_record(NULL::{qualified}, $1::jsonb))
             SELECT row_to_json(t)::jsonb FROM {qualified} t, o
             WHERE {key_match} FOR UPDATE OF t"
        );
        let current = transaction
            .query_opt(&lock_sql, &[&original_value])?
            .ok_or_else(|| anyhow!("row no longer exists"))?
            .get::<_, Value>(0);
        if current != original_value {
            bail!("row changed since it was opened; reload before saving");
        }
        let assignments = changed
            .iter()
            .map(|column| {
                let column = quote_pg_ident(column);
                format!("{column} = e.{column}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        let update_sql = format!(
            "WITH e AS (SELECT * FROM jsonb_populate_record(NULL::{qualified}, $1::jsonb)),
                  o AS (SELECT * FROM jsonb_populate_record(NULL::{qualified}, $2::jsonb))
             UPDATE {qualified} t SET {assignments} FROM e, o WHERE {key_match}"
        );
        let updated = transaction.execute(&update_sql, &[&edited_value, &original_value])?;
        if updated != 1 {
            bail!("row was not updated");
        }
        transaction.commit()?;
        Ok(())
    }

    fn mysql_update(
        &self,
        namespace: &str,
        table: &str,
        original: Map<String, Value>,
        edited: Map<String, Value>,
    ) -> Result<()> {
        let mut conn = self.mysql_conn()?;
        if self.is_read_only() {
            bail!("connection is read-only");
        }
        let (columns, kind) = self.mysql_columns(&mut conn, namespace, table)?;
        let changed = validate_edit(&columns, kind, &original, &edited)?;
        if changed.is_empty() {
            return Ok(());
        }
        let keys = primary_keys(&columns)?;
        let selected = columns
            .iter()
            .map(|column| quote_mysql_ident(&column.name))
            .collect::<Vec<_>>()
            .join(", ");
        let key_match = keys
            .iter()
            .map(|key| format!("{} <=> ?", quote_mysql_ident(key)))
            .collect::<Vec<_>>()
            .join(" AND ");
        let mut transaction = conn.start_transaction(mysql::TxOpts::default())?;
        let lock_sql = format!(
            "SELECT {selected} FROM {}.{} WHERE {key_match} FOR UPDATE",
            quote_mysql_ident(namespace),
            quote_mysql_ident(table)
        );
        let key_params = mysql_values_for(&columns, &keys, &original)?;
        let current: Option<mysql::Row> =
            transaction.exec_first(&lock_sql, mysql::Params::Positional(key_params.clone()))?;
        let current = current.ok_or_else(|| anyhow!("row no longer exists"))?;
        let current: Value = serde_json::from_str(&mysql_row_json(&columns, current)?)?;
        if current != Value::Object(original.clone()) {
            bail!("row changed since it was opened; reload before saving");
        }
        let assignments = changed
            .iter()
            .map(|column| format!("{} = ?", quote_mysql_ident(column)))
            .collect::<Vec<_>>()
            .join(", ");
        let mut params = mysql_values_for(&columns, &changed, &edited)?;
        params.extend(key_params);
        let update_sql = format!(
            "UPDATE {}.{} SET {assignments} WHERE {key_match}",
            quote_mysql_ident(namespace),
            quote_mysql_ident(table)
        );
        let result = transaction.exec_iter(update_sql, mysql::Params::Positional(params))?;
        if result.affected_rows() != 1 {
            bail!("row was not updated");
        }
        drop(result);
        transaction.commit()?;
        Ok(())
    }

    fn sqlite_update(
        &self,
        namespace: &str,
        table: &str,
        original: Map<String, Value>,
        edited: Map<String, Value>,
    ) -> Result<()> {
        let mut conn = self.sqlite_conn()?;
        let (columns, kind) = self.sqlite_columns(&conn, namespace, table)?;
        let changed = validate_edit(&columns, kind, &original, &edited)?;
        if changed.is_empty() {
            return Ok(());
        }
        let keys = primary_keys(&columns)?;
        let selected = columns
            .iter()
            .map(|column| quote_sqlite_ident(&column.name))
            .collect::<Vec<_>>()
            .join(", ");
        let key_match = keys
            .iter()
            .map(|key| format!("{} IS ?", quote_sqlite_ident(key)))
            .collect::<Vec<_>>()
            .join(" AND ");
        let transaction = conn.transaction()?;
        let select_sql = format!(
            "SELECT {selected} FROM {}.{} WHERE {key_match}",
            quote_sqlite_ident(namespace),
            quote_sqlite_ident(table)
        );
        let key_params = sqlite_values_for(&columns, &keys, &original)?;
        let current = {
            let mut statement = transaction.prepare(&select_sql)?;
            let mut rows = statement.query(rusqlite::params_from_iter(key_params.clone()))?;
            let row = rows
                .next()?
                .ok_or_else(|| anyhow!("row no longer exists"))?;
            serde_json::from_str::<Value>(&sqlite_row_json(&columns, row)?)?
        };
        if current != Value::Object(original.clone()) {
            bail!("row changed since it was opened; reload before saving");
        }
        let assignments = changed
            .iter()
            .map(|column| format!("{} = ?", quote_sqlite_ident(column)))
            .collect::<Vec<_>>()
            .join(", ");
        let mut params = sqlite_values_for(&columns, &changed, &edited)?;
        params.extend(key_params);
        let update_sql = format!(
            "UPDATE {}.{} SET {assignments} WHERE {key_match}",
            quote_sqlite_ident(namespace),
            quote_sqlite_ident(table)
        );
        let updated = transaction.execute(&update_sql, rusqlite::params_from_iter(params))?;
        if updated != 1 {
            bail!("row was not updated");
        }
        transaction.commit()?;
        Ok(())
    }
}

fn parse_row_object(json: &str) -> Result<Map<String, Value>> {
    match serde_json::from_str(json).context("row is not valid JSON")? {
        Value::Object(object) => Ok(object),
        _ => bail!("row must be a JSON object"),
    }
}

fn validate_filters(columns: &[TableColumn], filters: &[TableFilter]) -> Result<()> {
    for filter in filters {
        if !columns.iter().any(|column| column.name == filter.column) {
            bail!("unknown filter column: {}", filter.column);
        }
    }
    Ok(())
}

fn validate_edit(
    columns: &[TableColumn],
    kind: DbObjectKind,
    original: &Map<String, Value>,
    edited: &Map<String, Value>,
) -> Result<Vec<String>> {
    if kind != DbObjectKind::Table {
        bail!("views are read-only");
    }
    let keys = primary_keys(columns)?;
    for column in columns {
        if !original.contains_key(&column.name) || !edited.contains_key(&column.name) {
            bail!("edited row must retain every column");
        }
    }
    if edited
        .keys()
        .any(|key| !columns.iter().any(|column| &column.name == key))
    {
        bail!("edited row contains an unknown column");
    }
    for key in keys {
        if original.get(&key) != edited.get(&key) {
            bail!("primary-key column {key} cannot be changed");
        }
    }
    let mut changed = Vec::new();
    for column in columns {
        if original.get(&column.name) != edited.get(&column.name) {
            if !column.editable {
                bail!("generated column {} cannot be changed", column.name);
            }
            changed.push(column.name.clone());
        }
    }
    Ok(changed)
}

fn primary_keys(columns: &[TableColumn]) -> Result<Vec<String>> {
    let keys = columns
        .iter()
        .filter(|column| column.primary_key)
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    if keys.is_empty() {
        bail!("table has no primary key and is read-only");
    }
    Ok(keys)
}

fn append_order_by(sql: &mut String, columns: &[TableColumn], quote: fn(&str) -> String) {
    let keys = columns
        .iter()
        .filter(|column| column.primary_key)
        .map(|column| quote(&column.name))
        .collect::<Vec<_>>();
    if !keys.is_empty() {
        sql.push_str(" ORDER BY ");
        sql.push_str(&keys.join(", "));
    }
}

fn object_kind(value: &str) -> DbObjectKind {
    if value.to_ascii_lowercase().contains("view") {
        DbObjectKind::View
    } else {
        DbObjectKind::Table
    }
}

fn mysql_row_json(columns: &[TableColumn], row: mysql::Row) -> Result<String> {
    let values = row.unwrap();
    if values.len() != columns.len() {
        bail!("MySQL returned an unexpected column count");
    }
    let object = columns
        .iter()
        .zip(values)
        .map(|(column, value)| {
            (
                column.name.clone(),
                mysql_json_value(value, &column.data_type),
            )
        })
        .collect::<Map<_, _>>();
    Ok(serde_json::to_string_pretty(&Value::Object(object))?)
}

fn mysql_json_value(value: mysql::Value, data_type: &str) -> Value {
    match value {
        mysql::Value::NULL => Value::Null,
        mysql::Value::Bytes(value) => match String::from_utf8(value.clone()) {
            Ok(text) if data_type.eq_ignore_ascii_case("json") => {
                serde_json::from_str(&text).unwrap_or(Value::String(text))
            }
            Ok(text) => Value::String(text),
            Err(_) => Value::String(format!("0x{}", hex_encode(&value))),
        },
        mysql::Value::Int(value) => Value::Number(value.into()),
        mysql::Value::UInt(value) => Value::Number(value.into()),
        mysql::Value::Float(value) => Number::from_f64(value as f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        mysql::Value::Double(value) => Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        mysql::Value::Date(year, month, day, hour, minute, second, micros) => Value::String(
            format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{micros:06}"),
        ),
        mysql::Value::Time(negative, days, hours, minutes, seconds, micros) => {
            Value::String(format!(
                "{}{days} {hours:02}:{minutes:02}:{seconds:02}.{micros:06}",
                if negative { "-" } else { "" }
            ))
        }
    }
}

fn sqlite_row_json(columns: &[TableColumn], row: &rusqlite::Row<'_>) -> Result<String> {
    let mut object = Map::new();
    for (index, column) in columns.iter().enumerate() {
        let value = match row.get_ref(index)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(value) => Value::Number(value.into()),
            ValueRef::Real(value) => Number::from_f64(value)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            ValueRef::Text(value) => {
                let text = String::from_utf8_lossy(value).into_owned();
                if column.data_type.to_ascii_uppercase().contains("JSON") {
                    serde_json::from_str(&text).unwrap_or(Value::String(text))
                } else {
                    Value::String(text)
                }
            }
            ValueRef::Blob(value) => Value::String(format!("0x{}", hex_encode(value))),
        };
        object.insert(column.name.clone(), value);
    }
    Ok(serde_json::to_string_pretty(&Value::Object(object))?)
}

fn mysql_values_for(
    columns: &[TableColumn],
    names: &[String],
    object: &Map<String, Value>,
) -> Result<Vec<mysql::Value>> {
    names
        .iter()
        .map(|name| {
            let column = columns.iter().find(|column| &column.name == name).unwrap();
            mysql_bind_value(object.get(name).unwrap(), &column.data_type)
        })
        .collect()
}

fn mysql_bind_value(value: &Value, data_type: &str) -> Result<mysql::Value> {
    Ok(match value {
        Value::Null => mysql::Value::NULL,
        Value::Bool(value) => mysql::Value::Int(i64::from(*value)),
        Value::Number(value) if value.is_i64() => mysql::Value::Int(value.as_i64().unwrap()),
        Value::Number(value) if value.is_u64() => mysql::Value::UInt(value.as_u64().unwrap()),
        Value::Number(value) => mysql::Value::Double(
            value
                .as_f64()
                .ok_or_else(|| anyhow!("number cannot be represented"))?,
        ),
        Value::String(value) if is_binary_type(data_type) && value.strip_prefix("0x").is_some() => {
            mysql::Value::Bytes(hex_decode(value.trim_start_matches("0x"))?)
        }
        Value::String(value) => mysql::Value::Bytes(value.as_bytes().to_vec()),
        Value::Array(_) | Value::Object(_) => mysql::Value::Bytes(serde_json::to_vec(value)?),
    })
}

fn sqlite_values_for(
    columns: &[TableColumn],
    names: &[String],
    object: &Map<String, Value>,
) -> Result<Vec<SqliteValue>> {
    names
        .iter()
        .map(|name| {
            let column = columns.iter().find(|column| &column.name == name).unwrap();
            sqlite_bind_value(object.get(name).unwrap(), &column.data_type)
        })
        .collect()
}

fn sqlite_bind_value(value: &Value, data_type: &str) -> Result<SqliteValue> {
    Ok(match value {
        Value::Null => SqliteValue::Null,
        Value::Bool(value) => SqliteValue::Integer(i64::from(*value)),
        Value::Number(value) if value.is_i64() => SqliteValue::Integer(value.as_i64().unwrap()),
        Value::Number(value) if value.is_u64() => SqliteValue::Integer(
            i64::try_from(value.as_u64().unwrap()).context("integer too large")?,
        ),
        Value::Number(value) => SqliteValue::Real(
            value
                .as_f64()
                .ok_or_else(|| anyhow!("number cannot be represented"))?,
        ),
        Value::String(value) if is_binary_type(data_type) && value.strip_prefix("0x").is_some() => {
            SqliteValue::Blob(hex_decode(value.trim_start_matches("0x"))?)
        }
        Value::String(value) => SqliteValue::Text(value.clone()),
        Value::Array(_) | Value::Object(_) => SqliteValue::Text(serde_json::to_string(value)?),
    })
}

fn sqlite_path(uri: &str) -> Result<PathBuf> {
    let uri = uri.trim();
    if let Some(path) = uri.strip_prefix("sqlite://") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = uri.strip_prefix("sqlite:") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = uri.strip_prefix("file:") {
        return Ok(PathBuf::from(path.split('?').next().unwrap_or(path)));
    }
    Ok(Path::new(uri).to_path_buf())
}

fn pg_qualified(namespace: &str, table: &str) -> String {
    format!("{}.{}", quote_pg_ident(namespace), quote_pg_ident(table))
}

fn quote_pg_ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn quote_mysql_ident(value: &str) -> String {
    format!("`{}`", value.replace('`', "``"))
}

fn quote_sqlite_ident(value: &str) -> String {
    quote_pg_ident(value)
}

fn quote_sqlite_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn is_binary_type(data_type: &str) -> bool {
    let data_type = data_type.to_ascii_lowercase();
    data_type.contains("blob") || data_type.contains("binary") || data_type == "bytea"
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0xf) as usize] as char);
    }
    output
}

fn hex_decode(value: &str) -> Result<Vec<u8>> {
    if value.len() % 2 != 0 {
        bail!("binary hex value must contain an even number of digits");
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair)?;
            Ok(u8::from_str_radix(text, 16)?)
        })
        .collect()
}

fn redact(error: &(dyn std::error::Error + 'static)) -> String {
    let mut messages = Vec::new();
    let mut current = Some(error);

    // Driver display strings are often deliberately terse (for example,
    // postgres only reports "error performing TLS handshake"). Keep the
    // bounded source chain so the UI can show the actionable certificate or
    // network cause without risking an infinite chain from a broken error.
    while let Some(error) = current.take() {
        let message = error.to_string();
        if messages
            .last()
            .map_or(true, |previous| previous != &message)
        {
            messages.push(message);
        }
        if messages.len() >= 8 {
            break;
        }
        current = error.source();
    }

    crate::redaction::redact_sensitive_text(&messages.join(": "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_health_checks_reuse_one_session_across_cloned_handles() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut length = [0; 4];
            socket.read_exact(&mut length).unwrap();
            let mut startup = vec![0; u32::from_be_bytes(length) as usize - 4];
            socket.read_exact(&mut startup).unwrap();
            assert_eq!(&startup[..4], &196608_u32.to_be_bytes());
            assert!(startup.windows(5).any(|chunk| chunk == b"Choro"));
            // AuthenticationOk, BackendKeyData, ReadyForQuery.
            socket
                .write_all(&[
                    b'R', 0, 0, 0, 8, 0, 0, 0, 0, b'K', 0, 0, 0, 12, 0, 0, 0, 1, 0, 0, 0, 2, b'Z',
                    0, 0, 0, 5, b'I',
                ])
                .unwrap();
            for _ in 0..3 {
                let mut header = [0; 5];
                socket.read_exact(&mut header).unwrap();
                assert_eq!(header[0], b'Q');
                let mut query =
                    vec![0; u32::from_be_bytes(header[1..].try_into().unwrap()) as usize - 4];
                socket.read_exact(&mut query).unwrap();
                assert_eq!(query, b"SELECT 1\0");
                socket
                    .write_all(&[
                        b'C', 0, 0, 0, 13, b'S', b'E', b'L', b'E', b'C', b'T', b' ', b'1', 0, b'Z',
                        0, 0, 0, 5, b'I',
                    ])
                    .unwrap();
            }
            // Keep the stream alive until the client drops it. Closing directly
            // after ReadyForQuery can race the driver's response delivery.
            let mut termination = [0; 5];
            socket.read_exact(&mut termination).unwrap();
            assert_eq!(termination, [b'X', 0, 0, 0, 4]);
            listener.set_nonblocking(true).unwrap();
            assert!(
                matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
                "health checks opened more than one connection"
            );
        });
        let handle = SqlHandle::new(
            DbProvider::PostgreSql,
            format!("postgresql://fixture@{address}/fixture?sslmode=disable"),
        )
        .unwrap();
        handle.ping().unwrap();
        handle.clone().ping().unwrap();
        handle.ping().unwrap();
        drop(handle);
        server.join().unwrap();
    }

    #[test]
    fn sqlite_health_check_never_creates_a_missing_database() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing.sqlite");
        let handle =
            SqlHandle::new_with_access(DbProvider::SQLite, path.display().to_string(), true)
                .unwrap();
        assert!(handle.ping().is_err());
        assert!(!path.exists());
    }

    #[derive(Debug)]
    struct TestError {
        message: &'static str,
        source: Option<Box<TestError>>,
    }

    impl std::fmt::Display for TestError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.message)
        }
    }

    impl std::error::Error for TestError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.source
                .as_deref()
                .map(|source| source as &(dyn std::error::Error + 'static))
        }
    }

    #[test]
    fn sqlite_browse_filter_and_update() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("browser.sqlite");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE people (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                active INTEGER NOT NULL,
                profile JSON
             );
             INSERT INTO people VALUES (1, 'Ada', 1, '{\"role\":\"admin\"}');
             INSERT INTO people VALUES (2, 'Grace', 0, '{\"role\":\"user\"}');",
        )
        .unwrap();
        drop(conn);

        let handle = SqlHandle::new(DbProvider::SQLite, path.display().to_string()).unwrap();
        assert_eq!(handle.list_namespaces().unwrap(), vec!["main"]);
        assert_eq!(handle.list_objects("main").unwrap()[0].name, "people");
        let page = handle
            .fetch_rows(
                "main",
                "people",
                &[TableFilter {
                    column: "name".into(),
                    value: "Ad".into(),
                }],
                0,
                50,
            )
            .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert!(page.columns.iter().any(|column| column.primary_key));
        let original = page.rows[0].json.clone();
        let edited = original.replace("\"Ada\"", "\"Ada Lovelace\"");
        handle
            .update_row("main", "people", &original, &edited)
            .unwrap();
        let stale_edit = original.replace("\"Ada\"", "\"Countess\"");
        let conflict = handle
            .update_row("main", "people", &original, &stale_edit)
            .unwrap_err();
        assert!(conflict.to_string().contains("changed since"));
        let page = handle.fetch_rows("main", "people", &[], 0, 50).unwrap();
        assert!(page.rows[0].json.contains("Ada Lovelace"));

        let handle =
            SqlHandle::new_with_access(DbProvider::SQLite, path.display().to_string(), false)
                .unwrap();
        let open_pane_handle = handle.clone();
        handle.set_read_only(true);
        assert!(open_pane_handle.is_read_only());
        let page = open_pane_handle
            .fetch_rows("main", "people", &[], 0, 50)
            .unwrap();
        assert!(!page.editable);
        let error = open_pane_handle
            .update_row("main", "people", &page.rows[0].json, &page.rows[0].json)
            .unwrap_err();
        assert_eq!(error.to_string(), "connection is read-only");
    }

    #[test]
    fn supabase_transaction_pooler_is_rejected_with_guidance() {
        let error = SqlHandle::new(
            DbProvider::Supabase,
            "postgres://postgres.ref:secret@aws-0.pooler.supabase.com:6543/postgres",
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("Session pooler"));
    }

    #[test]
    fn connection_error_includes_redacted_source_chain() {
        let error = TestError {
            message: "error performing TLS handshake",
            source: Some(Box::new(TestError {
                message: "certificate rejected for postgresql://postgres:secret-value@db.example/postgres",
                source: None,
            })),
        };

        let diagnostic = redact(&error);
        assert!(diagnostic.contains("error performing TLS handshake"));
        assert!(diagnostic.contains("certificate rejected"));
        assert!(diagnostic.contains("[REDACTED]"));
        assert!(!diagnostic.contains("secret-value"));
    }

    #[test]
    fn hex_round_trip() {
        let bytes = b"\0binary\xff";
        assert_eq!(hex_decode(&hex_encode(bytes)).unwrap(), bytes);
    }

    #[test]
    fn mysql_value_conversion_preserves_text_json_and_binary_types() {
        assert_eq!(
            mysql_json_value(mysql::Value::Bytes(b"true".to_vec()), "varchar"),
            Value::String("true".into())
        );
        assert_eq!(
            mysql_json_value(mysql::Value::Bytes(br#"{"ok":true}"#.to_vec()), "json"),
            serde_json::json!({"ok": true})
        );
        let bound = mysql_bind_value(&Value::String("0x00ff".into()), "varbinary").unwrap();
        assert_eq!(bound, mysql::Value::Bytes(vec![0, 255]));
    }

    /// Exercises the same read-only connection path as the database modal:
    /// `CHORO_TEST_SUPABASE_URI=postgresql://... \
    ///  cargo test -p ide-core live_supabase_connection -- --ignored`
    #[test]
    #[ignore]
    fn live_supabase_connection() {
        let uri = std::env::var("CHORO_TEST_SUPABASE_URI")
            .expect("set CHORO_TEST_SUPABASE_URI to a Supabase Session pooler URL");
        let handle = SqlHandle::new(DbProvider::Supabase, uri).unwrap();
        let namespaces = handle.list_namespaces().unwrap();
        assert!(!namespaces.is_empty());
    }

    /// Uses a disposable schema in a developer-owned local PostgreSQL server:
    /// `CHORO_TEST_POSTGRES_URI=postgresql://user@localhost/postgres \
    ///  cargo test -p ide-core live_postgres_browse_and_edit -- --ignored`
    #[test]
    #[ignore]
    fn live_postgres_browse_and_edit() {
        let uri = std::env::var("CHORO_TEST_POSTGRES_URI")
            .expect("set CHORO_TEST_POSTGRES_URI to a disposable PostgreSQL database");
        let schema = format!("choro_test_{}", std::process::id());
        let handle = SqlHandle::new(DbProvider::PostgreSql, uri).unwrap();
        let mut client = handle.postgres_client().unwrap();
        client
            .batch_execute(&format!(
                "CREATE SCHEMA {schema};
                 CREATE TABLE {schema}.people (
                    id BIGINT PRIMARY KEY,
                    name TEXT NOT NULL,
                    profile JSONB NOT NULL
                 );
                 INSERT INTO {schema}.people VALUES
                    (1, 'Ada', '{{\"role\":\"admin\"}}'),
                    (2, 'Grace', '{{\"role\":\"user\"}}');"
            ))
            .unwrap();

        drop(client);

        let result = (|| {
            assert!(handle.list_namespaces()?.contains(&schema));
            assert_eq!(handle.list_objects(&schema)?[0].name, "people");
            let page = handle.fetch_rows(
                &schema,
                "people",
                &[TableFilter {
                    column: "name".into(),
                    value: "Ad".into(),
                }],
                0,
                50,
            )?;
            assert_eq!(page.rows.len(), 1);
            let original = page.rows[0].json.clone();
            let edited = original.replace("\"Ada\"", "\"Ada Lovelace\"");
            handle.update_row(&schema, "people", &original, &edited)?;
            let stale_edit = original.replace("\"Ada\"", "\"Countess\"");
            let conflict = handle
                .update_row(&schema, "people", &original, &stale_edit)
                .unwrap_err();
            assert!(conflict.to_string().contains("changed since"));
            let page = handle.fetch_rows(&schema, "people", &[], 0, 50)?;
            assert!(page.rows[0].json.contains("Ada Lovelace"));
            Ok::<_, anyhow::Error>(())
        })();

        handle
            .postgres_client()
            .unwrap()
            .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
            .unwrap();
        result.unwrap();
    }
}
