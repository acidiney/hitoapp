# Architecture decisions

## Capability services own application behavior

Identity, catalog, collection state, and indexing are separate concrete services, composed at startup. This keeps each service's rules and resource lifecycles together without a shared forwarding facade.

SQLite stays private to the core. Public tests use temporary real SQLite databases, not mocked repositories. Collection acceptance owns the complete write transaction. Individual action handlers receive that transaction and cannot commit it independently.

```mermaid
flowchart TB
  Host[Server or desktop host] --> Services[Service composition]
  Services --> Identity[Identity and authenticated actors]
  Services --> Catalog[Authorized catalog and source reads]
  Services --> Collection[Transactional personal changes]
  Services --> Indexer[Configured indexing and scan jobs]
  Identity --> Store[Private SQLite store]
  Catalog --> Store
  Collection --> Store
  Indexer --> Store
  Indexer --> Reader[Audio metadata and file reader]
```

The filesystem and SQLite are concrete local dependencies. Ports will be introduced with real external provider, synchronization transport, and native playback adapters. Empty interfaces for future features are not part of this refactor.

## Backend ownership differs from client identity

A backend host owns local files. A synchronization client originates operations. A browser can play a server-owned file through the authenticated relay; its client device ID must not become authority to read a filesystem path. Availability binds the configured backend host identity rather than embedding a server-specific string in queries.

## Contracts come from Rust

Annotated routes and Rust wire types generate OpenAPI. The same OpenAPI generates TypeScript. The server publishes that contract, and CI checks regeneration. Plugin protocol types remain separately versioned and Apache-2.0 licensed.

Typed Inertia props describe each page. HTTP handlers own transport details; they do not contain library rules. Authentication uses one extractor that returns an authenticated actor and checks the expected account header.

## React owns focused workflows

Catalog, likes, playlists, history, settings, and login are separate pages. Shared track rendering accepts typed data and actions. Scan polling owns cancellation. Mutation hooks preserve errors, so failures cannot trigger success navigation or discard form input.

Playback commands and position subscriptions live outside page rendering. Navigation preserves the player, and account changes dispose it and clear private history. The existing browser implementation remains a browser adapter; the native command/event contract will be designed alongside its first real adapter.

## Recording identity is independent of tags

A recording is a performance, a source supplies it, and a file locator describes local storage. Whole-file checksums change when tags change. The indexer stores exact file checksums separately from stable recording UUIDs and versioned decoded-audio evidence. An unambiguous verified audio match can preserve references after retagging; a reused path or similar title cannot. Historical ambiguous matches remain separate.

Bounded FFmpeg inspection reads a private owned input copy, with explicit cancellation, output limits, and child reaping. Source metadata observations survive independently of the selected display owner. SQLite identity resolution and metadata selection commit together. [Recording identity](recording-identity.md) documents the algorithm, upgrade witnesses, conservative exceptions, and tested formats.

## Logs follow workflow boundaries

Structured events and spans make requests, core operations, background jobs, and playback transitions observable. Logs share request IDs across HTTP and browser events. Frequent work uses lower levels. Safe error categories preserve operational evidence without serializing credentials, bodies, paths, or signed audio locations.
