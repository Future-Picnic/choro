//! Shared database-browser models and the synchronous MongoDB backend.
//! All calls block — run them on a background executor, never on the UI thread.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use anyhow::{Context as _, Result};
use mongodb::bson::{Bson, Document};
use mongodb::options::{ClientOptions, ConnectionString, FindOptions};
use mongodb::sync::Client;

use crate::{DbConnection, DbProvider, SqlHandle};

/// Server-selection timeout: fail fast instead of Mongo's 30s default.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Cheap to clone — the underlying client is a pooled handle.
#[derive(Clone)]
pub struct MongoHandle {
    client: Arc<Mutex<Option<Client>>>,
    uri: String,
    read_only: Arc<AtomicBool>,
}

/// One document from a collection, ready for display/editing.
pub struct DocEntry {
    /// The `_id` as canonical extended JSON — the handle for writes.
    /// None when the document somehow has no _id (not editable).
    pub id: Option<String>,
    /// Pretty-printed relaxed extended JSON of the whole document.
    pub json: String,
}

/// One page of documents from a collection.
pub struct DocPage {
    pub docs: Vec<DocEntry>,
    /// Total documents matching the filter (estimated when unfiltered).
    pub total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbObjectKind {
    Collection,
    Table,
    View,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbObject {
    pub name: String,
    pub kind: DbObjectKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub editable: bool,
    pub primary_key: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableFilter {
    pub column: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRow {
    pub json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TablePage {
    pub columns: Vec<TableColumn>,
    pub rows: Vec<TableRow>,
    pub has_more: bool,
    pub editable: bool,
}

#[derive(Clone)]
pub enum DatabaseHandle {
    Mongo(MongoHandle),
    Relational(SqlHandle),
}

impl DatabaseHandle {
    /// Identity of the live connection, independent of its editable access flag.
    /// Reopening a tab after reconnecting must bind it to the replacement session.
    pub fn shares_connection(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Mongo(left), Self::Mongo(right)) => {
                Arc::ptr_eq(&left.read_only, &right.read_only)
            }
            (Self::Relational(left), Self::Relational(right)) => left.shares_connection(right),
            _ => false,
        }
    }
    pub fn connect(connection: &DbConnection) -> Result<Self> {
        let uri = connection.expanded_uri()?;
        match connection.provider {
            DbProvider::MongoDb => Ok(Self::Mongo(MongoHandle::connect_with_access(
                &uri,
                connection.read_only,
            )?)),
            provider => Ok(Self::Relational(SqlHandle::new_with_access(
                provider,
                uri,
                connection.read_only,
            )?)),
        }
    }

    pub fn provider(&self) -> DbProvider {
        match self {
            Self::Mongo(_) => DbProvider::MongoDb,
            Self::Relational(handle) => handle.provider(),
        }
    }

    pub fn set_read_only(&self, read_only: bool) {
        match self {
            Self::Mongo(handle) => handle.set_read_only(read_only),
            Self::Relational(handle) => handle.set_read_only(read_only),
        }
    }

    /// A health probe that does not require schema discovery permissions.
    pub fn ping(&self) -> Result<()> {
        match self {
            Self::Mongo(handle) => handle.ping(),
            Self::Relational(handle) => handle.ping(),
        }
    }

    pub fn list_namespaces(&self) -> Result<Vec<String>> {
        match self {
            Self::Mongo(handle) => handle.list_databases(),
            Self::Relational(handle) => handle.list_namespaces(),
        }
    }

    pub fn list_objects(&self, namespace: &str) -> Result<Vec<DbObject>> {
        match self {
            Self::Mongo(handle) => Ok(handle
                .list_collections(namespace)?
                .into_iter()
                .map(|name| DbObject {
                    name,
                    kind: DbObjectKind::Collection,
                })
                .collect()),
            Self::Relational(handle) => handle.list_objects(namespace),
        }
    }
}

impl MongoHandle {
    pub fn connect(uri: &str) -> Result<Self> {
        Self::connect_with_access(uri, false)
    }

    fn connect_with_access(uri: &str, read_only: bool) -> Result<Self> {
        // Validate syntax without resolving SRV records on the UI thread.
        ConnectionString::parse(uri).map_err(|error| {
            anyhow::anyhow!(
                "invalid MongoDB connection string: {}",
                crate::redaction::redact_sensitive_text(&error.to_string())
            )
        })?;
        Ok(Self {
            client: Arc::new(Mutex::new(None)),
            uri: uri.to_owned(),
            read_only: Arc::new(AtomicBool::new(read_only)),
        })
    }

    fn client(&self) -> Result<Client> {
        let mut cached = self
            .client
            .lock()
            .map_err(|_| anyhow::anyhow!("MongoDB client lock failed"))?;
        if let Some(client) = cached.as_ref() {
            return Ok(client.clone());
        }
        let mut options = ClientOptions::parse(&self.uri).run().map_err(|error| {
            anyhow::anyhow!(
                "MongoDB connection failed: {}",
                crate::redaction::redact_sensitive_text(&error.to_string())
            )
        })?;
        options.server_selection_timeout = Some(CONNECT_TIMEOUT);
        options.connect_timeout = Some(CONNECT_TIMEOUT);
        let client = Client::with_options(options).context("failed to create MongoDB client")?;
        *cached = Some(client.clone());
        Ok(client)
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only.load(Ordering::Relaxed)
    }

    pub fn set_read_only(&self, read_only: bool) {
        self.read_only.store(read_only, Ordering::Relaxed);
    }

    pub fn ping(&self) -> Result<()> {
        self.client()?
            .database("admin")
            .run_command(mongodb::bson::doc! { "ping": 1 })
            .run()
            .map_err(|error| {
                anyhow::anyhow!(
                    "MongoDB connection failed: {}",
                    crate::redaction::redact_sensitive_text(&error.to_string())
                )
            })?;
        Ok(())
    }

    pub fn list_databases(&self) -> Result<Vec<String>> {
        let mut names = self
            .client()?
            .list_database_names()
            .run()
            .context("failed to list databases")?;
        names.retain(|n| !matches!(n.as_str(), "admin" | "local" | "config"));
        names.sort();
        Ok(names)
    }

    pub fn list_collections(&self, db: &str) -> Result<Vec<String>> {
        let mut names = self
            .client()?
            .database(db)
            .list_collection_names()
            .run()
            .context("failed to list collections")?;
        names.sort();
        Ok(names)
    }

    /// One page of documents. `filter_json` is a Mongo query like
    /// `{"status": "active"}`; empty means match-all.
    pub fn find_docs(
        &self,
        db: &str,
        collection: &str,
        filter_json: &str,
        skip: u64,
        limit: i64,
    ) -> Result<DocPage> {
        let coll = self
            .client()?
            .database(db)
            .collection::<Document>(collection);
        let filter = parse_filter(filter_json)?;

        let total = if filter.is_empty() {
            coll.estimated_document_count()
                .run()
                .context("failed to count documents")?
        } else {
            coll.count_documents(filter.clone())
                .run()
                .context("failed to count filtered documents")?
        };

        let options = FindOptions::builder().skip(skip).limit(limit).build();
        let cursor = coll
            .find(filter)
            .with_options(options)
            .run()
            .context("query failed")?;
        let docs = cursor
            .filter_map(|doc| doc.ok())
            .map(|doc| DocEntry {
                id: doc.get("_id").map(|id| {
                    serde_json::to_string(&id.clone().into_canonical_extjson()).unwrap_or_default()
                }),
                json: pretty_doc(doc),
            })
            .collect();
        Ok(DocPage { docs, total })
    }

    /// Replaces one document (matched by `_id`) with edited JSON.
    /// `id_canonical_json` comes from [`DocEntry::id`]; `new_json` is the
    /// (possibly edited) extended JSON. `_id` in the new body is ignored —
    /// Mongo forbids changing it.
    pub fn replace_doc(
        &self,
        db: &str,
        collection: &str,
        id_canonical_json: &str,
        new_json: &str,
    ) -> Result<()> {
        if self.is_read_only() {
            anyhow::bail!("connection is read-only");
        }
        let id_value: serde_json::Value =
            serde_json::from_str(id_canonical_json).context("bad document id")?;
        let id: Bson = id_value.try_into().context("bad document id")?;

        let value: serde_json::Value =
            serde_json::from_str(new_json).context("document is not valid JSON")?;
        let bson: Bson = value
            .try_into()
            .context("document is not valid extended JSON")?;
        let Bson::Document(mut new_doc) = bson else {
            anyhow::bail!("document must be a JSON object");
        };
        new_doc.remove("_id");

        let client = self.client()?;
        // Client initialization can block on SRV discovery. Recheck access
        // after that wait so a retired connection cannot begin a new write.
        if self.is_read_only() {
            anyhow::bail!("connection is read-only");
        }
        let result = client
            .database(db)
            .collection::<Document>(collection)
            .replace_one(mongodb::bson::doc! { "_id": id }, new_doc)
            .run()
            .context("write failed")?;
        anyhow::ensure!(
            result.matched_count == 1,
            "document no longer exists (was it deleted?)"
        );
        Ok(())
    }

    /// Deletes one document matched by its canonical extended-JSON `_id`.
    pub fn delete_doc(&self, db: &str, collection: &str, id_canonical_json: &str) -> Result<()> {
        if self.is_read_only() {
            anyhow::bail!("connection is read-only");
        }
        let id_value: serde_json::Value =
            serde_json::from_str(id_canonical_json).context("bad document id")?;
        let id: Bson = id_value.try_into().context("bad document id")?;

        let client = self.client()?;
        if self.is_read_only() {
            anyhow::bail!("connection is read-only");
        }
        let result = client
            .database(db)
            .collection::<Document>(collection)
            .delete_one(mongodb::bson::doc! { "_id": id })
            .run()
            .context("delete failed")?;
        anyhow::ensure!(
            result.deleted_count == 1,
            "document no longer exists (was it already deleted?)"
        );
        Ok(())
    }
}

fn parse_filter(filter_json: &str) -> Result<Document> {
    let trimmed = filter_json.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return Ok(Document::new());
    }
    let value: serde_json::Value = parse_filter_value(trimmed)?;
    let bson: Bson = value.try_into().context("filter is not a valid query")?;
    match bson {
        Bson::Document(doc) => Ok(doc),
        _ => anyhow::bail!("filter must be a JSON object"),
    }
}

fn parse_filter_value(trimmed: &str) -> Result<serde_json::Value> {
    serde_json::from_str(trimmed).or_else(|json_error| {
        json5::from_str(trimmed).with_context(|| {
            format!("filter is not valid JSON or JSON5-style query (JSON error: {json_error})")
        })
    })
}

fn pretty_doc(doc: Document) -> String {
    let value: serde_json::Value = Bson::Document(doc).into_relaxed_extjson();
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "<unrenderable document>".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mongo_srv_construction_is_lazy_and_validates_syntax_without_dns() {
        let handle =
            MongoHandle::connect("mongodb+srv://reader:password@does-not-exist.invalid/app")
                .unwrap();
        assert!(handle.client.lock().unwrap().is_none());
        assert!(MongoHandle::connect("not a database URL").is_err());
    }

    #[test]
    fn replacement_connections_have_new_identity_and_old_clones_can_be_revoked() {
        let connection = DbConnection::new_for(DbProvider::SQLite, "fixture", "existing.sqlite");
        let old = DatabaseHandle::connect(&connection).unwrap();
        let open_tab = old.clone();
        let replacement = DatabaseHandle::connect(&connection).unwrap();
        assert!(old.shares_connection(&open_tab));
        assert!(!old.shares_connection(&replacement));
        old.set_read_only(true);
        let DatabaseHandle::Relational(tab) = open_tab else {
            panic!("expected SQL")
        };
        assert!(tab.is_read_only());
        assert!(tab
            .update_row("main", "people", "{}", "{}")
            .unwrap_err()
            .to_string()
            .contains("read-only"));
        let DatabaseHandle::Relational(new) = replacement else {
            panic!("expected SQL")
        };
        assert!(!new.is_read_only());
    }

    #[test]
    fn empty_filter_matches_all() {
        assert!(parse_filter("").unwrap().is_empty());
        assert!(parse_filter(" {} ").unwrap().is_empty());
    }

    #[test]
    fn object_filter_parses() {
        let doc = parse_filter(r#"{"status": "active"}"#).unwrap();
        assert_eq!(doc.get_str("status").unwrap(), "active");
    }

    #[test]
    fn json5_style_filter_parses() {
        let doc = parse_filter(r#"{email:"asafelazari@gmail.com", active:true,}"#).unwrap();
        assert_eq!(doc.get_str("email").unwrap(), "asafelazari@gmail.com");
        assert_eq!(doc.get_bool("active").unwrap(), true);
    }

    #[test]
    fn non_object_filter_rejected() {
        assert!(parse_filter("[1,2]").is_err());
        assert!(parse_filter("not json").is_err());
    }

    #[test]
    fn read_only_mongo_handle_rejects_writes_before_network_access() {
        let handle = MongoHandle::connect_with_access("mongodb://localhost:27017", false).unwrap();
        let open_pane_handle = handle.clone();
        handle.set_read_only(true);
        assert!(open_pane_handle.is_read_only());
        let error = open_pane_handle
            .replace_doc(
                "db",
                "collection",
                r#"{"$oid":"000000000000000000000000"}"#,
                "{}",
            )
            .unwrap_err();
        assert_eq!(error.to_string(), "connection is read-only");
    }

    #[test]
    fn read_only_mongo_handle_rejects_deletes_before_network_access() {
        let handle = MongoHandle::connect_with_access("mongodb://localhost:27017", true).unwrap();
        let error = handle
            .delete_doc("db", "collection", r#"{"$oid":"000000000000000000000000"}"#)
            .unwrap_err();
        assert_eq!(error.to_string(), "connection is read-only");
    }

    /// Needs a local mongod with test.choro_smoke seeded (_id "smoke1"):
    /// `cargo test -p ide-core live_edit -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_edit_round_trip() {
        let handle = MongoHandle::connect("mongodb://localhost:27017").unwrap();
        let page = handle
            .find_docs("test", "choro_smoke", r#"{"_id": "smoke1"}"#, 0, 1)
            .unwrap();
        let entry = page.docs.first().expect("seed smoke1 first via mongosh");
        let id = entry.id.clone().unwrap();
        println!("before: {}", entry.json);

        let edited = entry.json.replace("\"before\"", "\"after\"");
        handle
            .replace_doc("test", "choro_smoke", &id, &edited)
            .unwrap();

        let page = handle
            .find_docs("test", "choro_smoke", r#"{"_id": "smoke1"}"#, 0, 1)
            .unwrap();
        let after = &page.docs.first().unwrap().json;
        println!("after: {after}");
        assert!(after.contains("\"after\""));

        // Restore so the test is re-runnable.
        let restored = after.replace("\"after\"", "\"before\"");
        handle
            .replace_doc("test", "choro_smoke", &id, &restored)
            .unwrap();
    }

    /// Needs a local mongod: `cargo test -p ide-core live_ -- --ignored`
    #[test]
    #[ignore]
    fn live_browse_local_mongo() {
        let handle = MongoHandle::connect("mongodb://localhost:27017").unwrap();
        let dbs = handle.list_databases().unwrap();
        println!("databases: {dbs:?}");
        if let Some(db) = dbs.first() {
            let colls = handle.list_collections(db).unwrap();
            println!("{db} collections: {colls:?}");
            if let Some(coll) = colls.first() {
                let page = handle.find_docs(db, coll, "", 0, 3).unwrap();
                println!("{}.{} total={} docs:", db, coll, page.total);
                for doc in &page.docs {
                    println!("{}", doc.json);
                }
            }
        }
    }
}
