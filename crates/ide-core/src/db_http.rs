//! Read-only HTTP database browsers. Transport is pooled and every request has
//! a deadline. Generated queries use bound values; no arbitrary SQL is accepted.
//!
//! Protocol references: libsql/docs/HTTP_V2_SPEC.md, turso/serverless/PROTOCOL.md,
//! and clickhouse.com/docs/interfaces/http. No upstream source is vendored.

use crate::{DbObject, DbObjectKind, DbProvider, TableColumn, TableFilter, TablePage, TableRow};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
use std::{io::Read, sync::Mutex, time::Duration};
use url::Url;

const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;

pub(crate) struct HttpSqlHandle {
    provider: DbProvider,
    endpoint: Url,
    user: String,
    password: String,
    token: String,
    database: Option<String>,
    client: Mutex<Option<reqwest::blocking::Client>>,
}

struct QueryRows {
    columns: Vec<(String, String)>,
    rows: Vec<Map<String, Value>>,
}

impl HttpSqlHandle {
    pub(crate) fn new(provider: DbProvider, uri: &str) -> Result<Self> {
        let normalized = if let Some(host) = uri.strip_prefix("libsql://") {
            format!("https://{host}")
        } else {
            uri.to_owned()
        };
        let mut endpoint =
            Url::parse(&normalized).map_err(|_| anyhow!("invalid HTTP database URL"))?;
        if !matches!(endpoint.scheme(), "http" | "https") || endpoint.host_str().is_none() {
            bail!("use a libsql://, https://, or local http:// database URL");
        }
        if endpoint.fragment().is_some() {
            bail!("database URL must not contain a fragment");
        }
        let user = percent_decoded(endpoint.username())?;
        let password = percent_decoded(endpoint.password().unwrap_or(""))?;
        let mut token = String::new();
        let mut protocol = "v2".to_owned();
        for (key, value) in endpoint.query_pairs() {
            match key.as_ref() {
                "authToken" | "auth_token" | "token" if provider == DbProvider::Turso => {
                    token = value.into_owned()
                }
                "protocol" if provider == DbProvider::Turso => protocol = value.into_owned(),
                _ => bail!("unsupported HTTP database URL option: {key}"),
            }
        }
        if provider == DbProvider::Turso && !user.is_empty() {
            bail!(
                "Turso authentication uses ?authToken=${{TURSO_AUTH_TOKEN}}, not URL user/password"
            );
        }
        if !matches!(protocol.as_str(), "v2" | "v3") {
            bail!("Turso protocol must be v2 or v3");
        }
        // Explicit loopback HTTP is useful for self-hosted protocol fixtures.
        // Credentials to remote services must retain normal HTTPS verification.
        let loopback = endpoint.host_str().is_some_and(|h| {
            h == "localhost"
                || h.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if endpoint.scheme() == "http"
            && !loopback
            && (!user.is_empty() || !password.is_empty() || !token.is_empty())
        {
            bail!("use HTTPS when sending database credentials to a remote host");
        }
        let path = endpoint.path().trim_matches('/');
        let database = if provider == DbProvider::ClickHouse && !path.is_empty() {
            Some(percent_decoded(path)?)
        } else {
            None
        };
        if provider == DbProvider::Turso && !path.is_empty() {
            bail!("Turso URL must name the database host without an API path");
        }
        endpoint
            .set_username("")
            .map_err(|_| anyhow!("invalid database URL username"))?;
        endpoint
            .set_password(None)
            .map_err(|_| anyhow!("invalid database URL password"))?;
        endpoint.set_query(None);
        endpoint.set_path(if provider == DbProvider::Turso {
            if protocol == "v3" {
                "/v3/pipeline"
            } else {
                "/v2/pipeline"
            }
        } else {
            "/"
        });
        Ok(Self {
            provider,
            endpoint,
            user,
            password,
            token,
            database,
            client: Mutex::new(None),
        })
    }

    fn client(&self) -> Result<reqwest::blocking::Client> {
        let mut cached = self
            .client
            .lock()
            .map_err(|_| anyhow!("HTTP database client lock failed"))?;
        if cached.is_none() {
            *cached = Some(
                reqwest::blocking::Client::builder()
                    .connect_timeout(Duration::from_secs(7))
                    .timeout(Duration::from_secs(30))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .context("failed to initialize HTTP database client")?,
            );
        }
        Ok(cached.as_ref().expect("initialized HTTP client").clone())
    }

    fn diagnostic(&self, message: &str) -> String {
        let mut text = crate::redaction::redact_sensitive_text(message);
        for secret in [&self.password, &self.token] {
            if !secret.is_empty() {
                text = text.replace(secret, "[REDACTED]");
            }
        }
        text
    }

    fn query(&self, sql: &str, args: &[Value]) -> Result<QueryRows> {
        let mut request = self.client()?.post(self.endpoint.clone());
        if self.provider == DbProvider::Turso {
            let args = args
                .iter()
                .map(|value| match value {
                    Value::String(text) => json!({"type":"text", "value":text}),
                    Value::Number(number) => json!({"type":"integer", "value":number.to_string()}),
                    _ => json!({"type":"null"}),
                })
                .collect::<Vec<_>>();
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(serde_json::to_vec(&json!({"baton":null, "requests":[
                    {"type":"execute", "stmt":{"sql":sql, "args":args, "want_rows":true}},
                    {"type":"close"}
                ]}))?);
            if !self.token.is_empty() {
                request = request.bearer_auth(&self.token);
            }
        } else {
            // Use the SQL parameter protocol, and enforce read-only at the server.
            request = request
                .query(&[
                    ("readonly", "1"),
                    ("max_execution_time", "30"),
                    ("wait_end_of_query", "1"),
                ])
                .body(format!("{sql} FORMAT JSON"));
            if let Some(database) = &self.database {
                request = request.query(&[("database", database)]);
            }
            for (index, value) in args.iter().enumerate() {
                let text = value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string());
                request = request.query(&[(format!("param_p{index}"), text)]);
            }
            if !self.user.is_empty() {
                request = request.basic_auth(&self.user, Some(&self.password));
            }
        }
        let response = request.send().map_err(|error| {
            anyhow!(
                "{} request failed: {}",
                self.provider.display_name(),
                self.diagnostic(&error.to_string())
            )
        })?;
        let status = response.status();
        let mut bytes = Vec::new();
        response
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                anyhow!(
                    "database response failed: {}",
                    self.diagnostic(&error.to_string())
                )
            })?;
        if bytes.len() as u64 > MAX_RESPONSE_BYTES {
            bail!("database response exceeded 16 MiB; use a narrower filter");
        }
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes);
            let detail: String = body.chars().take(2048).collect();
            bail!(
                "{} HTTP {}: {}",
                self.provider.display_name(),
                status.as_u16(),
                self.diagnostic(&detail)
            );
        }
        let value: Value =
            serde_json::from_slice(&bytes).context("database returned an invalid JSON response")?;
        if self.provider == DbProvider::Turso {
            self.decode_hrana(value)
        } else {
            decode_clickhouse(value)
        }
    }

    fn decode_hrana(&self, value: Value) -> Result<QueryRows> {
        let results = value
            .get("results")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("Turso response is missing pipeline results"))?;
        // Check errors from both execution and explicit stream close.
        for result in results {
            if result["type"] == "error" {
                bail!(
                    "Turso query failed: {}",
                    self.diagnostic(
                        result["error"]["message"]
                            .as_str()
                            .unwrap_or("unknown server error")
                    )
                );
            }
        }
        let result = results
            .first()
            .and_then(|r| r.get("response"))
            .and_then(|r| r.get("result"))
            .ok_or_else(|| anyhow!("Turso response is missing the statement result"))?;
        let columns = result["cols"]
            .as_array()
            .ok_or_else(|| anyhow!("Turso response is missing columns"))?
            .iter()
            .map(|column| {
                Ok((
                    column["name"]
                        .as_str()
                        .context("Turso column is missing its name")?
                        .to_owned(),
                    column["decltype"].as_str().unwrap_or("").to_owned(),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let rows = result["rows"]
            .as_array()
            .ok_or_else(|| anyhow!("Turso response is missing rows"))?
            .iter()
            .map(|row| {
                let cells = row.as_array().context("Turso row is not an array")?;
                if cells.len() != columns.len() {
                    bail!("Turso row does not match its column count");
                }
                columns
                    .iter()
                    .zip(cells)
                    .map(|((name, _), cell)| Ok((name.clone(), hrana_value(cell)?)))
                    .collect::<Result<Map<_, _>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(QueryRows { columns, rows })
    }

    pub(crate) fn ping(&self) -> Result<()> {
        self.query("SELECT 1", &[])?;
        Ok(())
    }

    pub(crate) fn list_namespaces(&self) -> Result<Vec<String>> {
        let result = if self.provider == DbProvider::Turso {
            self.query("PRAGMA database_list", &[])?
        } else {
            self.query("SELECT name FROM system.databases ORDER BY name", &[])?
        };
        result
            .rows
            .iter()
            .map(|row| string_field(row, "name"))
            .collect()
    }

    pub(crate) fn list_objects(&self, namespace: &str) -> Result<Vec<DbObject>> {
        let result = if self.provider == DbProvider::Turso {
            self.query(&format!("SELECT name, type FROM {}.sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name", quote_ident(namespace)), &[])?
        } else {
            self.query(
                "SELECT name, engine FROM system.tables WHERE database = {p0:String} ORDER BY name",
                &[json!(namespace)],
            )?
        };
        result
            .rows
            .iter()
            .map(|row| {
                let kind = if row.get("type").and_then(Value::as_str) == Some("view")
                    || row
                        .get("engine")
                        .and_then(Value::as_str)
                        .is_some_and(|engine| engine.ends_with("View"))
                {
                    DbObjectKind::View
                } else {
                    DbObjectKind::Table
                };
                Ok(DbObject {
                    name: string_field(row, "name")?,
                    kind,
                })
            })
            .collect()
    }

    pub(crate) fn fetch_rows(
        &self,
        namespace: &str,
        table: &str,
        filters: &[TableFilter],
        offset: u64,
        limit: u64,
    ) -> Result<TablePage> {
        if !(1..=500).contains(&limit) {
            bail!("page size must be between 1 and 500 rows");
        }
        let qualified = format!("{}.{}", quote_ident(namespace), quote_ident(table));
        let metadata = if self.provider == DbProvider::Turso {
            self.query(
                &format!(
                    "PRAGMA {}.table_xinfo('{}')",
                    quote_ident(namespace),
                    table.replace('\'', "''")
                ),
                &[],
            )?
        } else {
            self.query("SELECT name, type, is_in_primary_key FROM system.columns WHERE database = {p0:String} AND table = {p1:String} ORDER BY position", &[json!(namespace), json!(table)])?
        };
        let columns = metadata
            .rows
            .iter()
            .map(|row| {
                let data_type = string_field(row, "type")?;
                Ok(TableColumn {
                    name: string_field(row, "name")?,
                    nullable: if self.provider == DbProvider::Turso {
                        number_field(row, "notnull") == 0
                    } else {
                        data_type.starts_with("Nullable(")
                    },
                    primary_key: number_field(
                        row,
                        if self.provider == DbProvider::Turso {
                            "pk"
                        } else {
                            "is_in_primary_key"
                        },
                    ) > 0,
                    data_type,
                    editable: false,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if columns.is_empty() {
            bail!("table or view no longer exists");
        }
        let mut sql = format!(
            "SELECT {} FROM {qualified}",
            columns
                .iter()
                .map(|column| quote_ident(&column.name))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let mut args = Vec::new();
        for (index, filter) in filters.iter().enumerate() {
            if !columns.iter().any(|column| column.name == filter.column) {
                bail!("unknown filter column: {}", filter.column);
            }
            sql.push_str(if index == 0 { " WHERE " } else { " AND " });
            let column = quote_ident(&filter.column);
            if self.provider == DbProvider::Turso {
                sql.push_str(&format!("CAST({column} AS TEXT) LIKE ?"));
            } else {
                sql.push_str(&format!("toString({column}) LIKE {{p{index}:String}}"));
            }
            args.push(json!(format!("%{}%", filter.value)));
        }
        let keys = columns
            .iter()
            .filter(|column| column.primary_key)
            .map(|column| quote_ident(&column.name))
            .collect::<Vec<_>>();
        if !keys.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", keys.join(", ")));
        }
        sql.push_str(&format!(" LIMIT {} OFFSET {offset}", limit + 1));
        let result = self.query(&sql, &args)?;
        // Ensure that table metadata has not become stale between requests.
        if result
            .columns
            .iter()
            .map(|(name, _)| name)
            .ne(columns.iter().map(|column| &column.name))
        {
            bail!("table columns changed; refresh and try again");
        }
        let has_more = result.rows.len() > limit as usize;
        let rows = result
            .rows
            .into_iter()
            .take(limit as usize)
            .map(|row| {
                Ok(TableRow {
                    json: serde_json::to_string(&row)?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(TablePage {
            columns,
            rows,
            has_more,
            editable: false,
        })
    }
}

fn quote_ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn string_field(row: &Map<String, Value>, name: &str) -> Result<String> {
    row.get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("database metadata is missing {name}"))
}

fn number_field(row: &Map<String, Value>, name: &str) -> i64 {
    row.get(name)
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
        .unwrap_or(0)
}

fn hrana_value(cell: &Value) -> Result<Value> {
    match cell["type"].as_str() {
        Some("null") => Ok(Value::Null),
        Some("integer") => Ok(json!(cell["value"]
            .as_str()
            .context("invalid Turso integer")?
            .parse::<i64>()?)),
        Some("float") => cell["value"]
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .context("invalid Turso float"),
        Some("text") => Ok(json!(cell["value"]
            .as_str()
            .context("invalid Turso text")?)),
        Some("blob") => {
            Ok(json!({"base64":cell["base64"].as_str().context("invalid Turso blob")?}))
        }
        _ => bail!("unknown Turso value type"),
    }
}

fn decode_clickhouse(value: Value) -> Result<QueryRows> {
    let columns = value["meta"]
        .as_array()
        .context("ClickHouse response is missing column metadata")?
        .iter()
        .map(|column| {
            Ok((
                column["name"]
                    .as_str()
                    .context("ClickHouse column is missing its name")?
                    .to_owned(),
                column["type"]
                    .as_str()
                    .context("ClickHouse column is missing its type")?
                    .to_owned(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let rows = value["data"]
        .as_array()
        .context("ClickHouse response is missing data")?
        .iter()
        .map(|row| {
            row.as_object()
                .cloned()
                .context("ClickHouse row is not an object")
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(QueryRows { columns, rows })
}

// URL credentials and database path components use percent escapes, not form
// encoding: a literal '+' must stay '+'. Url::query_pairs handles form values.
fn percent_decoded(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut result = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let escape = bytes
                .get(index + 1..index + 3)
                .context("invalid URL percent escape")?;
            result.push(
                u8::from_str_radix(std::str::from_utf8(escape)?, 16)
                    .context("invalid URL percent escape")?,
            );
            index += 3;
        } else {
            result.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(result).context("URL credentials must be valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DatabaseHandle, DbConnection, SqlHandle};
    use std::{io::Write, net::TcpListener, thread};

    fn fixture(responses: Vec<(u16, String)>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in responses {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "fixture request timed out"
                            );
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("fixture accept: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 4096];
                    let n = stream.read(&mut chunk).unwrap();
                    assert!(n > 0, "fixture request ended early");
                    bytes.extend_from_slice(&chunk[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]);
                        let length: usize = header
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                requests.push(String::from_utf8(bytes).unwrap());
                let location = if status == 302 {
                    "Location: http://127.0.0.1:1/\r\n"
                } else {
                    ""
                };
                write!(stream, "HTTP/1.1 {status} Fixture\r\n{location}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        (url, server)
    }

    fn ok(value: Value) -> (u16, String) {
        (200, value.to_string())
    }

    fn hrana(names: &[&str], rows: Vec<Vec<Value>>) -> Value {
        let rows = rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|cell| match cell {
                        Value::Null => json!({"type":"null"}),
                        Value::Number(n) => json!({"type":"integer", "value":n.to_string()}),
                        Value::String(s) => json!({"type":"text", "value":s}),
                        _ => panic!("unsupported fixture value"),
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        json!({"baton":null, "results":[
            {"type":"ok", "response":{"type":"execute", "result":{
                "cols":names.iter().map(|name| json!({"name":name,"decltype":"TEXT"})).collect::<Vec<_>>(), "rows":rows}}},
            {"type":"ok", "response":{"type":"close"}}
        ]})
    }

    fn clickhouse(names: &[(&str, &str)], rows: Value) -> Value {
        json!({"meta":names.iter().map(|(name,kind)| json!({"name":name,"type":kind})).collect::<Vec<_>>(),"data":rows})
    }

    #[test]
    fn turso_http_browse_binds_filters_closes_streams_and_preserves_large_integers() {
        let (url, server) = fixture(vec![
            ok(hrana(&["1"], vec![vec![json!(1)]])),
            ok(hrana(
                &["seq", "name", "file"],
                vec![vec![json!(0), json!("main"), json!("")]],
            )),
            ok(hrana(
                &["name", "type"],
                vec![vec![json!("people"), json!("table")]],
            )),
            ok(hrana(
                &["name", "type", "notnull", "pk"],
                vec![
                    vec![json!("id"), json!("INTEGER"), json!(1), json!(1)],
                    vec![json!("name"), json!("TEXT"), json!(0), json!(0)],
                ],
            )),
            ok(hrana(
                &["id", "name"],
                vec![
                    vec![json!(9007199254740993_i64), json!("Ada")],
                    vec![json!(2), Value::Null],
                ],
            )),
        ]);
        let connection = DbConnection::new_for(
            DbProvider::Turso,
            "fixture",
            format!("{url}?authToken=fixture-token&protocol=v3"),
        );
        let handle = DatabaseHandle::connect(&connection).unwrap();
        handle.ping().unwrap();
        assert_eq!(handle.list_namespaces().unwrap(), ["main"]);
        assert_eq!(handle.list_objects("main").unwrap()[0].name, "people");
        let DatabaseHandle::Relational(sql) = handle else {
            panic!("expected SQL handle")
        };
        sql.set_read_only(false);
        assert!(sql.is_read_only());
        let page = sql
            .fetch_rows(
                "main",
                "people",
                &[TableFilter {
                    column: "name".into(),
                    value: "' OR 1=1 --".into(),
                }],
                0,
                1,
            )
            .unwrap();
        assert!(page.has_more);
        assert!(!page.editable);
        assert_eq!(
            serde_json::from_str::<Value>(&page.rows[0].json).unwrap()["id"],
            json!(9007199254740993_i64)
        );
        assert!(sql
            .update_row("main", "people", "{}", "{}")
            .unwrap_err()
            .to_string()
            .contains("read-only"));
        let requests = server.join().unwrap();
        for request in &requests {
            assert!(request.starts_with("POST /v3/pipeline "));
            assert!(request
                .to_lowercase()
                .contains("authorization: bearer fixture-token"));
            let body: Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body["requests"][1]["type"], "close");
        }
        let query: Value =
            serde_json::from_str(requests[4].split_once("\r\n\r\n").unwrap().1).unwrap();
        assert!(!query["requests"][0]["stmt"]["sql"]
            .as_str()
            .unwrap()
            .contains("OR 1=1"));
        assert_eq!(
            query["requests"][0]["stmt"]["args"][0]["value"],
            "%' OR 1=1 --%"
        );
    }

    #[test]
    fn clickhouse_http_browse_authenticates_and_enforces_read_only() {
        let (url, server) = fixture(vec![
            ok(clickhouse(&[("1", "UInt8")], json!([{"1":1}]))),
            ok(clickhouse(&[("name", "String")], json!([{"name":"app"}]))),
            ok(clickhouse(
                &[("name", "String"), ("engine", "String")],
                json!([{"name":"people","engine":"MergeTree"}]),
            )),
            ok(clickhouse(
                &[
                    ("name", "String"),
                    ("type", "String"),
                    ("is_in_primary_key", "UInt8"),
                ],
                json!([
                {"name":"id","type":"UInt64","is_in_primary_key":1},
                {"name":"name","type":"Nullable(String)","is_in_primary_key":0}]),
            )),
            ok(clickhouse(
                &[("id", "UInt64"), ("name", "Nullable(String)")],
                json!([{"id":"18446744073709551615","name":null}]),
            )),
        ]);
        let uri = url.replacen("http://", "http://reader:pa%2Bss@", 1) + "/app";
        let sql = SqlHandle::new(DbProvider::ClickHouse, uri).unwrap();
        sql.ping().unwrap();
        assert_eq!(sql.list_namespaces().unwrap(), ["app"]);
        assert_eq!(sql.list_objects("app").unwrap()[0].name, "people");
        let page = sql
            .fetch_rows(
                "app",
                "people",
                &[TableFilter {
                    column: "name".into(),
                    value: "Ada's".into(),
                }],
                0,
                50,
            )
            .unwrap();
        assert!(page.columns[1].nullable);
        assert!(!page.editable);
        assert!(page.rows[0].json.contains("18446744073709551615"));
        let requests = server.join().unwrap();
        for request in &requests {
            assert!(request.contains("readonly=1"));
            assert!(request.contains("database=app"));
            assert!(request
                .to_lowercase()
                .contains("authorization: basic cmvhzgvy"));
        }
        assert!(requests[4].contains("param_p0=%25Ada%27s%25"));
        assert!(requests[4].contains("toString(\"name\") LIKE {p0:String}"));
        assert!(!requests[4]
            .split_once("\r\n\r\n")
            .unwrap()
            .1
            .contains("Ada's"));
    }

    #[test]
    fn http_database_errors_redact_credentials_and_do_not_follow_redirects() {
        let (url, server) = fixture(vec![(401, "rejected super-secret-token".into())]);
        let sql = SqlHandle::new(
            DbProvider::Turso,
            format!("{url}?authToken=super-secret-token"),
        )
        .unwrap();
        let error = sql.ping().unwrap_err().to_string();
        assert!(error.contains("401"));
        assert!(!error.contains("super-secret-token"));
        server.join().unwrap();
        let (url, server) = fixture(vec![(302, "redirect".into())]);
        let sql = SqlHandle::new(DbProvider::Turso, url).unwrap();
        assert!(sql.ping().unwrap_err().to_string().contains("302"));
        server.join().unwrap();
    }

    #[test]
    fn http_urls_validate_credentials_and_protocol_without_network() {
        assert!(
            HttpSqlHandle::new(DbProvider::Turso, "libsql://example.turso.io?authToken=x").is_ok()
        );
        assert!(
            HttpSqlHandle::new(DbProvider::Turso, "http://remote.example?authToken=x").is_err()
        );
        assert!(
            HttpSqlHandle::new(DbProvider::Turso, "https://example.turso.io?protocol=v9").is_err()
        );
        assert!(
            HttpSqlHandle::new(DbProvider::ClickHouse, "https://host?password=secret").is_err()
        );
        assert_eq!(percent_decoded("pa%2Bss+word").unwrap(), "pa+ss+word");
        assert!(percent_decoded("bad%xx").is_err());
    }

    #[test]
    fn hrana_decoding_preserves_null_float_blob_and_rejects_malformed_values() {
        assert_eq!(hrana_value(&json!({"type":"null"})).unwrap(), Value::Null);
        assert_eq!(
            hrana_value(&json!({"type":"float","value":1.25})).unwrap(),
            json!(1.25)
        );
        assert_eq!(
            hrana_value(&json!({"type":"blob","base64":"AP8="})).unwrap(),
            json!({"base64":"AP8="})
        );
        assert!(hrana_value(&json!({"type":"integer","value":"oops"})).is_err());
        assert!(hrana_value(&json!({"type":"text"})).is_err());
    }
}
