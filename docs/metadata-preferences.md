# Account metadata preferences

The verified source exposes account-owned metadata preferences through Settings and the versioned API. New accounts use the built-in MusicBrainz provider by default. A listener can disable provider use, select approved personal catalog connections, remove providers and change their order. Changes require an explicit Save. This source is not yet deployed on the normal development server.

These preferences control the reviewed-profile host. Automatic enrichment, artist search, preview and confirmation pages, discography traversal and album-edition track lists remain unfinished. Opening Settings does not request catalog descriptors or profiles and does not import or download media. File tags and personal metadata edits remain separate.

## Ownership and updates

```mermaid
flowchart LR
    Page[Settings Metadata tab] --> Hook[Account workflow and editable draft]
    Hook --> API[Authenticated GET and PUT metadata settings]
    API --> Core[Core catalog capability]
    Core --> Store[Private SQLite policy and revision]
    Hook --> Connections[Approved personal plugin list]
    Core --> Profile[Reviewed profile access checks]
```

`GET /api/v1/metadata/settings` returns `revision`, `enabled` and the ordered `providers`. `PUT` accepts those same fields, with the revision last read by the caller. An accepted update advances the revision. A stale revision returns a conflict and preserves the accepted policy. The interface retains the unsaved draft after a failed save; Reload explicitly reads the current policy before another attempt.

Providers are the built-in `builtin_musicbrainz` or `http_plugin` with an opaque personal `installation_id`. An enabled policy requires one to eight distinct providers. Approved catalog capability and current account authority are checked in core. Offered services alone do not authorize personal use. Two connections with the same display name retain distinct identities.

A removed connection may remain referenced by a saved policy. The interface labels it unavailable. Disabling provider use preserves those references and succeeds without contacting the removed service. Re-enabling an unauthorized reference fails without replacing the accepted disabled policy. Removing a provider from the policy does not disconnect its plugin.

## Request lifetime and privacy

The workflow loads the policy and personal plugin list together. Account changes and unmount cancel owned requests and prevent late results from publishing into another account. Save is single flight. A canceled observer cannot silently retry a mutation with newer data. The server authenticates the shared account extractor and applies the normal browser origin and request-header checks.

Structured workflow events include a generated request ID, action, outcome and elapsed time. They exclude account and installation identifiers, service addresses, credentials, titles and search terms. Generated OpenAPI and TypeScript use the registered Rust handlers and core wire types.

## Verification and remaining work

Four owned HTTP cases pass with real temporary SQLite and a local catalog fixture. They cover the built-in default, exact saved personal provider, account isolation, stale revisions, disconnected references, anonymous and mismatched-account denial, browser origin checks and zero catalog-data requests. Both accounts retain their private edits, duplicate playlist entries, likes and progress. The scoped core TRACE case verifies conflict correlation and redaction.

All 298 frontend cases pass, including nine new preference cases. They cover explicit saving, provider identity and ordering, the eight-provider limit, missing references, StrictMode, canceled reads, account changes, concurrent saves and explicit conflict reload. The real Settings component supports the Metadata deep link and existing keyboard tab navigation. Both TypeScript checks and the production frontend build pass.

Packaged Chromium/WebKit acceptance, combined server/workspace checks, independent source review and deployment qualification are still in progress. These source checks do not establish live metadata availability, physical-device compatibility or public-release performance.
