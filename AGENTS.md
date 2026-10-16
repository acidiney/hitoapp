# Working on Hito

The active scope is docs/release-plan.md. docs/status.md records evidence and unfinished release work. Keep the full scope intact.

## Execution

Run application builds and tests through Docker Compose. Host tools may edit and inspect files. Use the tools profile for Rust/Node and the e2e profile for the isolated browser server. Never point first-run browser tests at the normal persistent server volume.

Give source snapshots and worktrees separate Cargo target directories. Mounting different source trees at `/workspace` with a shared target can reuse stale timestamp-based artifacts.

Keep Docker development/test debug information and incremental compilation disabled. Run compiler-heavy checks sequentially. Check host free space and Hito target sizes before and during large builds: retain at least 40 GiB of host headroom and keep combined Hito Cargo targets within 10 GiB. If either bound is reached, stop starting builds and reclaim only inactive, regenerable Hito output. Never use a global Docker prune or remove another project's artifacts or application data.

## Structure

- Core capability services own identity, catalog, collection transactions, and indexing. Service composition contains no business forwarding methods.
- SQLite, SQL, migrations, filesystem ownership, and transaction rules stay private to core.
- HTTP adapters authenticate through the shared extractor, map typed requests/results, and implement transport policy.
- Generated OpenAPI and TypeScript come from actual Rust routes and wire types. Do not hand-maintain a second API model.
- React pages own one route. Workflow hooks own async lifecycles and cancellation. Shared components accept typed data/actions. Page rendering does not subscribe to playback position ticks.
- Keep files cohesive and small. Split by responsibility when a file mixes workflows; avoid empty wrapper layers, per-table repository traits, and one-file-per-function fragmentation.
- Backend host identity and synchronization client identity are different concepts.

## Verification

Use TDD for behavior changes. Test through public capability/HTTP/browser boundaries with real temporary SQLite and owned deterministic audio fixtures. Test external services at their protocol boundary when adapters exist. Preserve transaction, privacy, identity, and recovery invariants when refactoring.

Review security, correctness, performance, and architectural ownership before accepting a slice. A green narrow test does not prove a release gate.

## Code and logs

Prefer clear names and explicit types. Keep code comments minimal; comments should not justify tangled control flow. Log every workflow with structured spans/events, safe outcomes, correlation IDs, and useful timings. Frequent work belongs at debug/trace. Never log credentials, session material, raw bodies, audio, private paths, search terms, titles, or signed URLs.

## Documentation

Use plain language and maintain diagrams where they explain ownership or data flow. Distinguish current behavior, tested evidence, planned behavior, and external dependencies. Never describe a local development deployment as a completed public release.

## Commits

Make small, focused commits with single-line messages. Do not sign commits. Disable signing for each commit without changing the user's global Git settings.
