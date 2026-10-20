# Automatic media cleanup

Automatic cleanup is planned. Its policy must cover Hito-managed music, podcast and audiobook copies and associated media, preserve user-owned originals and every explicit Download retention, and allow users to disable it. The default setting and storage budget are not implemented.

## Current storage ownership

| Data | Current owner and retention |
| --- | --- |
| Original local media | Library folders. Disconnecting a folder withdraws availability without deleting files. |
| Saved audio | Explicit recording or playlist Download requests. Shared retention references protect copies until all requests are removed. |
| Browser and native playback | Audio is streamed; browser-engine buffering is separate from Hito's explicit downloads. The YouTube service keeps source descriptors in memory and disables extractor disk caching. There is no Hito-owned durable unpinned audio-cache pool to evict. |
| Recording, release and playlist artwork | Account-owned SQLite blobs with shared references. Current admission limits are 32 MiB per account and 256 MiB globally; pruning removes unreferenced blobs. |
| Desktop WebView data | Browser-engine storage that can include private application state. It is not a directory of owned media copies. |

Current download reclamation removes only owned UUID/attempt filenames after fencing writers and inspection. It does not provide automatic-cache reader leases. Evicting a ready source without those leases could interrupt current playback or a later range request. The current listening-progress row records a cursor and speed, not a qualified play-frequency count. Counting progress writes would favor long recordings and frequent checkpoints.

## Required implementation boundaries

An automatic copy needs separate residency and authority from an explicit Download. Reclamation must recheck current readers, shared references and explicit retentions before deleting bytes, preserve catalog records and progress, and permit later retrieval through an eligible provider. Provider persistence restrictions still apply. A budget must report protected bytes and explain when protected or active copies prevent it from being met.

Usage ranking needs bounded private evidence of playback frequency and recency rather than selections or checkpoint count. The acceptance workflow must exercise cold and frequently played copies, shared and explicit downloads, active readers, disk pressure, disabled cleanup, restart and on-demand recovery across media types.

Artwork needs a separate residency adapter. Removing SQLite blobs can make database pages reusable, but does not itself prove that the operating system reclaimed disk space. Database compaction and physical reclamation require their own verification. No cleanup implementation should claim disk savings from logical row deletion alone.
