# Choro Demo bundle

`Choro Demo.app` uses the production `choro` and `choro-mcp` binaries with an isolated application identifier and data directory. Every launch recreates the same four fictional projects from source-controlled fixtures and seeds Choro through the normal persistence APIs.

## Build and run

```sh
CHORO_DEMO_INSTALL_TO_APPLICATIONS=0 scripts/bundle-demo.sh
open "target/release/bundle/Choro Demo.app"
```

## Reset behavior

- A newly built Demo bundle receives a unique build identifier.
- Every launch removes only `~/Library/Application Support/com.ritmus.choro.demo` and reseeds it.
- Every launch also uses fresh Demo-only installation and Design Keychain namespaces.
- Production Choro data is never read, copied, or removed.

## Jira

Northstar Client Portal contains a Jira connection for `Kan board`, filtered to `Liran Gabai`. The Demo app reads the optional token from:

```text
~/Library/Application Support/com.ritmus.choro.demo-secrets/jira-token
```

Alternatively, provide `CHORO_DEMO_JIRA_TOKEN`. During a reset, the seed resolves the board and assignee account IDs. The generated Choro database stores `${CHORO_DEMO_JIRA_TOKEN}`, not the raw credential.

`CHORO_DEMO_JIRA_BOARD_ID` can be supplied to bypass board-name discovery.

## Seeded workspace

- Products
  - Northstar Client Portal
  - Momentum Habit Tracker
- Client Work
  - Ember Coffee Website
- Services
  - Relay Orders API

Each project contains Docs, personal tasks, an in-progress agent chat plus completed chat history, local references, environment files, scripts, and a real Git repository with prepared history. Safe `.env` files are generated from the tracked `.env.example` fixtures during seeding. Northstar is favorited and active by default.
