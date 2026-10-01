# Database browser: DBFlux research and adoption

Research baseline: [DBFlux commit 3141fb0](https://github.com/0xErwin1/dbflux/tree/3141fb0b66fbbc10011aee48b85d31d9ee61f7d1), reviewed September 30, 2026. Choro's saved UI Designer uses Opus 5.5 through the managed Choro teammate system.

## What to adopt

| Upstream evidence | Decision for Choro |
| --- | --- |
| [Connection manager](https://github.com/0xErwin1/dbflux/blob/3141fb0b66fbbc10011aee48b85d31d9ee61f7d1/crates/dbflux_core/src/connection/manager.rs) retains live connections and owns metadata caches. | Share one lazy PostgreSQL session across cloned browser handles and retain a shared MySQL pool. Keep transactions exclusively checked out; use that same connection for table metadata. Reconnect a closed PostgreSQL session before the next operation without replaying failed writes. |
| [Driver contracts](https://github.com/0xErwin1/dbflux/blob/3141fb0b66fbbc10011aee48b85d31d9ee61f7d1/crates/dbflux_core/src/core/traits.rs) distinguish health probes from schema retrieval. | Add `DatabaseHandle::ping` and backend probes. A successful credential test should not require schema-listing privileges. |
| [Driver capabilities](https://github.com/0xErwin1/dbflux/blob/3141fb0b66fbbc10011aee48b85d31d9ee61f7d1/crates/dbflux_core/src/driver/capabilities.rs) describe actual supported operations. | Introduce `DbProvider::ALL` and `DbCapabilities` for relational/document browsing, file selection, editable rows, and default access. Advertise only implemented providers. |
| [Connection manager UI](https://github.com/0xErwin1/dbflux/tree/3141fb0b66fbbc10011aee48b85d31d9ee61f7d1/crates/dbflux_ui_windows/src/connection_manager) separates provider selection, connection fields, and connection status. | Adapt its workflow to Choro's existing modal, compact controls, masked credentials, and actionable errors. Preserve the saved connection identity and project scope. |
| [Data table](https://github.com/0xErwin1/dbflux/tree/3141fb0b66fbbc10011aee48b85d31d9ee61f7d1/crates/dbflux_components/src/components/data_table) aligns cells and headers and shares horizontal scrolling. | Use a dense relational grid with coherent header/body alignment, row inspection, filtering and pagination, while retaining MongoDB's document view. |
| [Supported drivers](https://github.com/0xErwin1/dbflux/blob/3141fb0b66fbbc10011aee48b85d31d9ee61f7d1/docs/DRIVERS.md) explicitly document per-driver limitations. | Add actual Turso/libSQL and ClickHouse HTTP browsing using the existing HTTP dependency. Both are read-only in this implementation. |

## What to avoid importing

- The complete upstream UI or driver workspace: its driver crates depend on DBFlux-specific core, SSH and orchestration contracts. Importing them wholesale would make Choro maintain two application architectures.
- PostgreSQL URI TLS behavior that accepts invalid certificates under `prefer`/`require`. Choro retains certificate and hostname verification.
- Its full external-driver RPC, AWS authentication, hook, scripting, audit and dashboard systems. Those are separate product changes with considerably more dependencies.
- Capability labels without working operations. SQL Server, Redis, DynamoDB and other DBFlux drivers are not automatically supported by Choro.
- Query retry after an uncertain write. A lost response does not mean a write failed to commit.

This implementation uses the upstream architecture and UX as research; it does not vendor upstream source. DBFlux is dual MIT/Apache-2.0 licensed. If future changes copy source, preserve the applicable attribution and license notices.

## New HTTP connection formats

- Turso/libSQL: `libsql://database.turso.io?authToken=${TURSO_AUTH_TOKEN}`. `libsql://` selects HTTPS. The default is the libSQL HTTP v2 protocol; use `&protocol=v3` for a Turso endpoint serving the newer protocol. Local `http://localhost:8080` endpoints are also supported.
- ClickHouse: `https://reader:${CLICKHOUSE_PASSWORD}@host:8443/database`, or an unauthenticated local `http://localhost:8123/database`. URL user/password values must be percent-encoded when they contain reserved characters. The path selects the default database; schema browsing can select another database allowed by the account.

Remote credentials require HTTPS with normal certificate verification. HTTP redirects are disabled so authentication cannot move to another endpoint. Clients reuse HTTP connections, apply a 7-second connection deadline and a 30-second request deadline, and reject responses exceeding 16 MiB. Turso explicitly closes its per-request server stream. ClickHouse also sets server-side read-only mode and an execution deadline. Generated filters bind values instead of interpolating user text; identifiers are quoted.

Official protocol references: [libSQL HTTP v2](https://github.com/tursodatabase/libsql/blob/main/docs/HTTP_V2_SPEC.md), [Turso serverless protocol](https://github.com/tursodatabase/turso/blob/main/serverless/PROTOCOL.md), [ClickHouse HTTP interface](https://clickhouse.com/docs/concepts/features/interfaces/http).

## Connection lifecycle and interface

The supported providers are MongoDB, PostgreSQL, Supabase, SQLite, MySQL, MariaDB, Turso/libSQL and ClickHouse. Turso and ClickHouse are read-only; the existing providers keep their current row/document editing contracts.

The connection editor groups all eight providers by capability, masks credentials, checks environment-variable availability, and reports health-probe timing with retry. The schema tree loads lazily and rejects stale responses. The grid aligns headers and rows in one horizontal scroll plane, distinguishes NULL and numeric cells, and retains its last successful page after a failed request. Apply commits a filter draft; pagination and Refresh use the committed query. Reconnecting or changing an endpoint revokes writes through retired handles, and reopening an object replaces any tab attached to the former session.

MongoDB client construction is lazy, including SRV resolution, so opening the tree does not block the UI thread. Row writes recheck access after client checkout. Existing production confirmations remain in place.

Long paths truncate in the context bar and remain readable in tab tooltips. Paging controls retain their width. Headless tests cover 560–1100 px panes, long identifiers, production indicators, horizontal header/body alignment, independent vertical row scrolling and inspector geometry.

## Database workspace layout

The center Database view is a document workspace modeled on DBFlux's, built from Choro's own tokens and controls:

- A tab strip leads with a pinned Connections tab. Each open table, view or collection gets its own tab with the provider mark, a production glyph when relevant, and a close control on the tab.
- The Connections home lists every saved connection with its provider, a credential-free endpoint, access posture and live state. From the home you can connect, retry or reconnect, choose a namespace, and open objects straight into tabs. It reads and drives the side explorer's state, so the two always agree. A connection with a single namespace opens it automatically.
- A context bar above the open object shows connection / namespace / object and reads write access live from the session, so revoked access shows in tabs that are already open.
- Tables and collections share a query bar, with `WHERE` for SQL `column=value` filters and `find` for Mongo, and a status bar showing row or document counts, columns, filters, fetch time and paging. SQL values match substrings; this is not an arbitrary SQL editor.
- Tables have a row inspector for reading wide rows one field per line with types. Editing still goes through the keyed row editor and production confirmation.
- Collections show a document list (`_id` and a field preview) beside the selected document's expandable tree. "Edit document" opens the existing guarded editor. A failed refresh keeps the page on screen.
- The side explorer gives each connection two lines (state and provider), puts a state dot on the provider mark, groups objects into Tables, Views and Collections when a namespace mixes kinds, and highlights the object on screen.

Choro's saved UI Designer (Opus 5.5) designed and implemented this replacement directly, including the center workspace and Mongo browser. Connection settings reconcile independently of sidebar rendering and across all projects. Stale clicks cannot reopen an edited or removed endpoint. Mongo, like SQL, commits its displayed page and filter only after a successful current request; failures retain results, selection and context, and Retry repeats the failed request. Closing a background tab preserves the active tab or Connections home.

## Verification and limits

Combined validation on October 1, 2026:

```sh
cargo test -p ide-core
cargo test -p ide-app --bin choro --features ui-layout-tests
cargo build -p ide-app --bin choro
```

The final core suite passed 452 unit tests and nine integration tests, with four opt-in tests ignored. The final app suite passed 885 tests, with four ignored, including 39 tests focused on database UI and center tab selection. The local app and MCP builds, scoped Rust formatting and `git diff --check` passed. Existing unrelated/vendor warnings remain.

A signed development preview is staged under `target/debug/bundle/db-redesign-58db8b52.*/Choro.app`, with a README beside it. It contains the fresh app and MCP binaries plus the installed app's existing runtime resources and frameworks, and passed deep, strict signature verification using the installed app's Developer ID identity. The installed app was not replaced or restarted. Quit the current Choro and open the preview to see the redesigned UI with the normal local workspace data. The preview includes other existing shared working-copy changes.

Local HTTP fixtures exercise authentication, health probes, namespace/table discovery, metadata, filtering, pagination, large integers, typed/null values, error redaction, redirect rejection and explicit stream cleanup. A local PostgreSQL wire fixture verifies that cloned handles use one network session. Existing SQLite browse/edit/conflict tests cover the persistent-session refactor's shared models; missing-file health tests ensure probing does not create databases.

No user database was queried and no live Choro state database was inspected. Live PostgreSQL/Supabase tests remain opt-in. The HTTP fixtures validate the implemented protocol contracts; they do not certify compatibility with every hosted server version or account permission profile. UI checks include source review, the app test suite, and headless GPUI tests of real table/collection entities, hidden-sidebar connection sync and native layout. Center tab selection uses tested production selection functions; the full RootView/CenterArea click and deferred-notification path has source review, not an end-to-end native test. Colors and rendered pixels remain unverified because no live app control was authorized.

Current limits: no arbitrary SQL editor, cross-provider query history, SSH/proxy setup, Redis/SQL Server drivers, or new query-cancellation UI. HTTP rows are not editable. Pagination of tables without a unique ordering key can change under concurrent writes. PostgreSQL operations on one saved connection share a serialized session; independent saved connections remain independent.
