//! Provider metadata for the database UI: picker grouping, form copy, and
//! safe connection summaries. Everything here is driven by `DbProvider` and
//! its declared capabilities so a new backend only needs one entry per helper.

use ide_core::DbProvider;

/// A titled group in the provider picker. Grouping follows what Choro can
/// actually do with a provider rather than marketing categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProviderSection {
    /// Relational databases whose keyed rows can be edited when allowed.
    Relational,
    /// Relational services browsed over HTTP; always read-only in Choro.
    BrowseOnly,
    /// Document databases edited as JSON by `_id`.
    Document,
}

impl ProviderSection {
    pub(super) const ORDER: [Self; 3] = [Self::Relational, Self::BrowseOnly, Self::Document];

    pub(super) fn for_provider(provider: DbProvider) -> Self {
        let capabilities = provider.capabilities();
        if !capabilities.relational {
            Self::Document
        } else if capabilities.row_editing {
            Self::Relational
        } else {
            Self::BrowseOnly
        }
    }

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Relational => "Relational",
            Self::BrowseOnly => "Browse only",
            Self::Document => "Document",
        }
    }

    pub(super) fn description(self) -> &'static str {
        match self {
            Self::Relational => "Browse schemas and tables. Keyed rows can be edited when allowed.",
            Self::BrowseOnly => "Browse tables over HTTPS. Choro never writes to these services.",
            Self::Document => "Browse collections and edit JSON documents by _id.",
        }
    }

    pub(super) fn providers(self) -> impl Iterator<Item = DbProvider> {
        DbProvider::ALL
            .into_iter()
            .filter(move |provider| Self::for_provider(*provider) == self)
    }
}

/// Short monospace hint on a picker card: the port, scheme, or `file`.
pub(super) fn provider_hint(provider: DbProvider) -> &'static str {
    match provider {
        DbProvider::MongoDb => ":27017",
        DbProvider::PostgreSql => ":5432",
        DbProvider::Supabase => ":5432 direct",
        DbProvider::SQLite => "file",
        DbProvider::MySql => ":3306",
        DbProvider::MariaDb => ":3306",
        DbProvider::Turso => "libsql://",
        DbProvider::ClickHouse => ":8443 https",
    }
}

pub(super) fn provider_placeholder(provider: DbProvider) -> &'static str {
    match provider {
        DbProvider::MongoDb => "mongodb+srv://user:${MONGO_PASSWORD}@cluster.example.net",
        DbProvider::PostgreSql => "postgresql://user:${POSTGRES_PASSWORD}@host:5432/database",
        DbProvider::Supabase => {
            "postgresql://postgres:${SUPABASE_DB_PASSWORD}@db.project.supabase.co:5432/postgres"
        }
        DbProvider::SQLite => "/absolute/path/to/database.sqlite",
        DbProvider::MySql => "mysql://user:${MYSQL_PASSWORD}@host:3306/database",
        DbProvider::MariaDb => "mysql://user:${MARIADB_PASSWORD}@host:3306/database",
        DbProvider::Turso => "libsql://database.turso.io?authToken=${TURSO_AUTH_TOKEN}",
        DbProvider::ClickHouse => "https://reader:${CLICKHOUSE_PASSWORD}@host:8443/database",
    }
}

pub(super) fn provider_name_placeholder(provider: DbProvider) -> &'static str {
    match provider {
        DbProvider::MongoDb => "e.g. Production MongoDB",
        DbProvider::PostgreSql => "e.g. Staging PostgreSQL",
        DbProvider::Supabase => "e.g. Product Supabase",
        DbProvider::SQLite => "e.g. Local app database",
        DbProvider::MySql => "e.g. Analytics MySQL",
        DbProvider::MariaDb => "e.g. Production MariaDB",
        DbProvider::Turso => "e.g. Edge Turso database",
        DbProvider::ClickHouse => "e.g. Events ClickHouse",
    }
}

pub(super) fn provider_connection_label(provider: DbProvider) -> &'static str {
    match provider {
        DbProvider::MongoDb => "MongoDB connection string",
        DbProvider::PostgreSql => "PostgreSQL connection string",
        DbProvider::Supabase => "Supabase Direct connection URL",
        DbProvider::SQLite => "SQLite database file",
        DbProvider::MySql => "MySQL connection string",
        DbProvider::MariaDb => "MariaDB connection string",
        DbProvider::Turso => "Turso database URL",
        DbProvider::ClickHouse => "ClickHouse HTTP URL",
    }
}

pub(super) fn provider_form_description(provider: DbProvider) -> &'static str {
    match provider {
        DbProvider::MongoDb => "Browse databases and edit JSON documents by _id.",
        DbProvider::PostgreSql => "Browse schemas, tables, and views on any PostgreSQL server.",
        DbProvider::Supabase => "Connect a Supabase project with its Direct connection URL.",
        DbProvider::SQLite => "Open an existing SQLite file on this Mac.",
        DbProvider::MySql => "Browse MySQL databases, tables, and views.",
        DbProvider::MariaDb => "Browse MariaDB databases, tables, and views.",
        DbProvider::Turso => "Browse Turso and libSQL tables over HTTPS. Read-only.",
        DbProvider::ClickHouse => "Browse ClickHouse databases and tables over HTTP. Read-only.",
    }
}

pub(super) fn provider_connection_help(provider: DbProvider) -> &'static str {
    match provider {
        DbProvider::MongoDb => {
            "Paste a mongodb:// or mongodb+srv:// URI. Keep passwords in env vars such as ${MONGO_PASSWORD}."
        }
        DbProvider::PostgreSql => {
            "Use a postgres:// or postgresql:// URL. Keep passwords in env vars such as ${POSTGRES_PASSWORD}."
        }
        DbProvider::Supabase => {
            "In Supabase, open Connect, select Direct connection, and paste its port 5432 URL. The port 6543 transaction pooler is not supported."
        }
        DbProvider::SQLite => {
            "Choose an existing .sqlite, .sqlite3, or .db file. Choro will not create a missing file."
        }
        DbProvider::MySql => {
            "Use a mysql:// URL. Keep passwords in env vars such as ${MYSQL_PASSWORD}."
        }
        DbProvider::MariaDb => {
            "Use a mysql:// URL for MariaDB. Keep passwords in env vars such as ${MARIADB_PASSWORD}."
        }
        DbProvider::Turso => {
            "Use the libsql:// or https:// database URL with authToken=${TURSO_AUTH_TOKEN}. Add protocol=v3 for endpoints that require it."
        }
        DbProvider::ClickHouse => {
            "Use the HTTP interface URL: https on 8443, or http://localhost:8123 for a local server. Keep passwords in ${CLICKHOUSE_PASSWORD}."
        }
    }
}

pub(super) fn provider_help_title(provider: DbProvider) -> &'static str {
    match provider {
        DbProvider::MongoDb => "Connect MongoDB",
        DbProvider::PostgreSql => "Connect PostgreSQL",
        DbProvider::Supabase => "Connect Supabase",
        DbProvider::SQLite => "Open SQLite",
        DbProvider::MySql => "Connect MySQL",
        DbProvider::MariaDb => "Connect MariaDB",
        DbProvider::Turso => "Connect Turso",
        DbProvider::ClickHouse => "Connect ClickHouse",
    }
}

pub(super) fn provider_help_steps(provider: DbProvider) -> &'static [&'static str] {
    match provider {
        DbProvider::MongoDb => &[
            "Copy the connection string from MongoDB Atlas or your MongoDB server.",
            "Paste it above, choose the access mode, then test the connection.",
        ],
        DbProvider::PostgreSql => &[
            "Copy a PostgreSQL connection URL that includes the host, port, user, and database.",
            "Keep Read only enabled until you intentionally want primary-key row editing.",
        ],
        DbProvider::Supabase => &[
            "Open the Supabase project dashboard and choose Connect.",
            "In the Connect dialog, select Direct connection.",
            "Copy the URI on port 5432, replace [YOUR-PASSWORD], paste it above, and test it.",
        ],
        DbProvider::SQLite => &[
            "Choose a database file already present on this Mac.",
            "Allow edits only when Choro should write directly to that local file.",
        ],
        DbProvider::MySql => &[
            "Copy a MySQL URL containing the host, port, user, password, and database.",
            "Keep Read only enabled until you intentionally want primary-key row editing.",
        ],
        DbProvider::MariaDb => &[
            "Copy a MariaDB connection as a mysql:// URL with its host, credentials, and database.",
            "Keep Read only enabled until you intentionally want primary-key row editing.",
        ],
        DbProvider::Turso => &[
            "Run turso db show --url <database> to get the libsql:// URL.",
            "Create a read-only token with turso db tokens create <database> --read-only.",
            "Export it as TURSO_AUTH_TOKEN before launching Choro and reference it as ${TURSO_AUTH_TOKEN}.",
        ],
        DbProvider::ClickHouse => &[
            "Use a ClickHouse user limited to SELECT on the databases you want to browse.",
            "Paste the HTTP interface URL with the database as its path.",
            "Export the password before launching Choro and reference it as ${CLICKHOUSE_PASSWORD}.",
        ],
    }
}

pub(super) fn access_help(provider: DbProvider, read_only: bool) -> &'static str {
    if !provider.capabilities().row_editing {
        return "Choro only reads from this provider. Edit actions are never shown.";
    }
    if read_only {
        return "Browsing and filtering are allowed. Choro will not show edit actions.";
    }
    if provider == DbProvider::MongoDb {
        "Documents with an _id can be edited. Production connections ask for a second confirmation."
    } else {
        "Tables with a primary key can be edited. Views and keyless tables stay read-only."
    }
}

/// The effective access for a provider: browse-only providers never allow writes.
pub(super) fn effective_read_only(provider: DbProvider, read_only: bool) -> bool {
    read_only || !provider.capabilities().row_editing
}

/// Capability facts shown under the provider name in the connection form.
pub(super) fn capability_facts(provider: DbProvider) -> Vec<&'static str> {
    let capabilities = provider.capabilities();
    let mut facts = Vec::new();
    facts.push(if capabilities.relational {
        "Tables and views"
    } else {
        "Collections"
    });
    facts.push(if !capabilities.row_editing {
        "Read-only"
    } else if capabilities.relational {
        "Primary-key row editing"
    } else {
        "Document editing"
    });
    if capabilities.local_file {
        facts.push("Local file");
    }
    if provider.is_http() {
        facts.push("HTTPS API");
    }
    facts
}

/// A credential-free `host:port/database` (or file name) for list rows. Only
/// the authority and path survive; userinfo and query strings (which carry
/// passwords and tokens) are always dropped.
pub(super) fn endpoint_summary(provider: DbProvider, uri: &str) -> Option<String> {
    let uri = uri.trim();
    if uri.is_empty() {
        return None;
    }
    if provider.capabilities().local_file {
        let path = uri.strip_prefix("file:").unwrap_or(uri);
        let name = std::path::Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        return Some(name);
    }
    // Incomplete draft text may be a pasted secret, not an endpoint. Never
    // surface it outside the masked input without a recognizable URL scheme.
    let (scheme, rest) = uri.split_once("://")?;
    if !matches!(
        scheme,
        "mongodb"
            | "mongodb+srv"
            | "postgres"
            | "postgresql"
            | "mysql"
            | "libsql"
            | "http"
            | "https"
    ) {
        return None;
    }
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if host.is_empty() {
        return None;
    }
    for authority in host.split(',') {
        let address = url::Url::parse(&format!("http://{authority}/")).ok()?;
        if address.host_str().is_none()
            || !address.username().is_empty()
            || address.password().is_some()
        {
            return None;
        }
    }
    let host = match host.split_once(',') {
        Some((first, others)) => format!("{first} +{}", others.split(',').count()),
        None => host.to_string(),
    };
    let database = path.trim_matches('/');
    Some(if database.is_empty() {
        host
    } else {
        format!("{host}/{database}")
    })
}

/// A `${NAME}` reference in a connection string and whether Choro's process
/// environment defines it. Values are never read into the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EnvReference {
    pub name: String,
    pub defined: bool,
}

pub(super) fn env_references(uri: &str) -> Vec<EnvReference> {
    env_reference_names(uri)
        .into_iter()
        .map(|name| EnvReference {
            defined: std::env::var_os(&name).is_some(),
            name,
        })
        .collect()
}

fn env_reference_names(uri: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut remainder = uri;
    while let Some(start) = remainder.find("${") {
        let after = &remainder[start + 2..];
        let Some(end) = after.find('}') else {
            break;
        };
        let name = after[..end].trim();
        if !name.is_empty() && !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
        remainder = &after[end + 1..];
    }
    names
}

/// True when the connection string embeds a literal password instead of a
/// `${VAR}` reference, so the form can suggest moving it out of the config.
pub(super) fn has_inline_secret(provider: DbProvider, uri: &str) -> bool {
    if provider.capabilities().local_file {
        return false;
    }
    let rest = uri.split_once("://").map_or(uri, |(_, rest)| rest);
    let (authority, query) = match rest.split_once('?') {
        Some((before, query)) => (before.split('/').next().unwrap_or_default(), query),
        None => (rest.split('/').next().unwrap_or_default(), ""),
    };
    let password_inline = authority
        .rsplit_once('@')
        .and_then(|(userinfo, _)| userinfo.split_once(':'))
        .is_some_and(|(_, password)| !password.is_empty() && !password.starts_with("${"));
    let token_inline = query.split('&').any(|pair| {
        pair.split_once('=').is_some_and(|(key, value)| {
            let key = key.to_ascii_lowercase();
            (key.contains("token") || key.contains("password"))
                && !value.is_empty()
                && !value.starts_with("${")
        })
    });
    password_inline || token_inline
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_provider_has_distinct_form_copy_and_a_section() {
        let labels = DbProvider::ALL
            .into_iter()
            .map(provider_connection_label)
            .collect::<HashSet<_>>();
        assert_eq!(labels.len(), DbProvider::ALL.len());
        let sectioned = ProviderSection::ORDER
            .into_iter()
            .flat_map(ProviderSection::providers)
            .count();
        assert_eq!(sectioned, DbProvider::ALL.len());
        assert!(provider_connection_help(DbProvider::Supabase).contains("port 5432"));
        assert!(provider_connection_help(DbProvider::Supabase).contains("6543"));
        assert_eq!(
            provider_help_steps(DbProvider::Supabase)[1],
            "In the Connect dialog, select Direct connection."
        );
        assert_eq!(
            provider_connection_label(DbProvider::SQLite),
            "SQLite database file"
        );
    }

    #[test]
    fn http_providers_are_grouped_and_forced_read_only() {
        assert_eq!(
            ProviderSection::for_provider(DbProvider::Turso),
            ProviderSection::BrowseOnly
        );
        assert_eq!(
            ProviderSection::for_provider(DbProvider::ClickHouse),
            ProviderSection::BrowseOnly
        );
        assert_eq!(
            ProviderSection::for_provider(DbProvider::MongoDb),
            ProviderSection::Document
        );
        assert!(effective_read_only(DbProvider::ClickHouse, false));
        assert!(!effective_read_only(DbProvider::SQLite, false));
        assert!(provider_placeholder(DbProvider::Turso).contains("${TURSO_AUTH_TOKEN}"));
        assert!(capability_facts(DbProvider::Turso).contains(&"Read-only"));
    }

    #[test]
    fn endpoint_summary_never_includes_credentials() {
        assert_eq!(
            endpoint_summary(
                DbProvider::PostgreSql,
                "postgresql://app:hunter2@db.example.com:5432/orders?sslmode=require"
            )
            .as_deref(),
            Some("db.example.com:5432/orders")
        );
        assert_eq!(
            endpoint_summary(
                DbProvider::Turso,
                "libsql://edge.turso.io?authToken=secret-token"
            )
            .as_deref(),
            Some("edge.turso.io")
        );
        assert_eq!(
            endpoint_summary(
                DbProvider::MongoDb,
                "mongodb://u:p@a.example:27017,b.example:27017,c.example:27017/app"
            )
            .as_deref(),
            Some("a.example:27017 +2/app")
        );
        assert_eq!(
            endpoint_summary(DbProvider::SQLite, "/Users/me/data/app.sqlite").as_deref(),
            Some("app.sqlite")
        );
        assert_eq!(endpoint_summary(DbProvider::MySql, "  "), None);
        assert_eq!(
            endpoint_summary(DbProvider::PostgreSql, "pasted-secret-token"),
            None
        );
        assert_eq!(
            endpoint_summary(
                DbProvider::PostgreSql,
                "postgresql://user:unescaped/secret@host/db"
            ),
            None
        );
    }

    #[test]
    fn env_references_are_unique_and_ordered() {
        assert_eq!(
            env_reference_names("postgres://u:${PG_PASS}@${PG_HOST}/db?x=${PG_PASS}"),
            vec!["PG_PASS".to_string(), "PG_HOST".to_string()]
        );
        assert!(env_reference_names("postgres://u:${UNCLOSED@host").is_empty());
    }

    #[test]
    fn inline_secrets_are_detected_but_env_references_are_not() {
        assert!(has_inline_secret(
            DbProvider::PostgreSql,
            "postgresql://app:hunter2@host/db"
        ));
        assert!(!has_inline_secret(
            DbProvider::PostgreSql,
            "postgresql://app:${PG_PASSWORD}@host/db"
        ));
        assert!(has_inline_secret(
            DbProvider::Turso,
            "libsql://edge.turso.io?authToken=abc"
        ));
        assert!(!has_inline_secret(
            DbProvider::Turso,
            "libsql://edge.turso.io?authToken=${TURSO_AUTH_TOKEN}"
        ));
        assert!(!has_inline_secret(DbProvider::SQLite, "/tmp/a:b@c.db"));
    }
}
