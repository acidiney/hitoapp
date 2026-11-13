# Design proposals

Each RFC explains one design change: the problem, what must stay true, the options we considered and the one we chose. Implemented RFCs also say what we did and what's still imperfect.

| RFC | Topic | Status |
| --- | --- | --- |
| [0001](0001-provider-source-lifecycle.md) | One owner for provider sources | Proposed |
| [0002](0002-playback-access-boundary.md) | Give playback access its own owner | Implemented |
| [0003](0003-page-read-models.md) | Move page data decisions into the core | Implemented |
| [0004](0004-host-and-sync-client-identity.md) | Tell host identity and sync client identity apart | Proposed |

[Architecture](../architecture.md) gives the background.

## Writing an RFC

Write for a person, not a compiler. Explain the problem and its cost, the rules that must not break, two or three options with their trade-offs, and a recommendation. Leave out code, function signatures and line numbers unless an idea can't be understood without them. Add a diagram when it shows who owns what.
