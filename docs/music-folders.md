# Music folders

Administrators manage folders in **Settings → Library**. Adding a folder queues its first scan. Each folder has status, rescan, and removal controls. Choose a library first when an account has several. Ordinary listeners do not request folder administration data.

## Choose folders through the file explorer

Run **Hito Setup.command** on macOS, **Hito Setup.ps1** on Windows, or `./hito-setup` on Linux. Docker must be running. Linux needs Zenity or KDialog. Windows rejects directory links and asks for the original folder.

Select locations to share. macOS supports multiple selections, Windows offers another selection after each folder, and Linux can add another location by running setup again. Existing locations remain shared. Setup builds the server in Docker, generates read-only mounts, retains recognized existing data, and archives the stopped data volume before restarting.

In Hito, select **Settings → Library → Add music folder**, browse a shared location, and choose **Use this folder**. Repeat for other locations. No environment-variable editing is required.

```mermaid
flowchart LR
  Picker[Native host folder chooser] --> Setup[Host setup launcher]
  Setup --> Compose[Read-only Docker mounts]
  Setup --> Manifest[Installation manifest]
  Compose --> Grants[Server directory grants]
  Manifest --> Grants
  Admin[Administrator Settings] --> Browser[Browse granted directories]
  Grants --> Browser
  Browser --> Folders[Connected library folders]
  Folders --> Jobs[Independent durable scans]
  Jobs --> Library[Recordings and sources]
```

The browser can select only locations already shared with the server. It cannot open the server computer's native dialog or grant Docker access to another drive. Run setup on the server computer to share another location. The application has no Docker socket or Docker-control API.

Setup files live in `~/Library/Application Support/Hito` on macOS, `%LOCALAPPDATA%\Hito` on Windows, and `$XDG_CONFIG_HOME/hito` or `~/.config/hito` on Linux. The private installation record contains host paths and stable grant IDs. The server manifest contains container paths. Administrator API responses expose labels and relative selections, without host paths. The host configuration directory is private; its manifest is readable by the container user.

Setup discovers server containers using their repository label, or uses its saved project. More than one candidate stops setup before configuration changes. Unsupported network settings or mounts also stop adoption. Moving repositories/configuration and native platform recovery remain installation acceptance work.

## Desktop Music folder and native chooser

The standalone desktop app runs the same Settings screen against its bundled local backend. It adds two helpers for administrators. Browsers and the Docker server show neither.

- **Proposal.** In **Settings → Library**, Hito offers "Add your Music folder?" when the computer has a readable Music folder that is not already connected. Nothing is added until you choose **Use Music folder**. **Not now** hides the offer until the page is opened again. New installations reach this screen from the empty library's **Add music folder** button.
- **Choose folder…** opens the operating system's folder chooser. A cancelled chooser changes nothing. A chosen folder is connected through the same add-folder command as the in-app browser, so scans, overlap checks, removal and recovery are unchanged.

The Music folder is the standard one for the platform: the Windows Music known folder, `~/Music` on macOS, and the XDG user-directory setting on Linux. An absolute `XDG_MUSIC_DIR` environment value takes priority, and `~/Music` is the last fallback. A missing, non-directory or unreadable folder is not proposed.

```mermaid
flowchart LR
  Settings[Settings page] --> Offer[GET host folders]
  Offer --> Adapter[Desktop shell adapter]
  Adapter --> Find[Platform Music folder check]
  Settings --> Choose[POST choose folder]
  Choose --> Adapter
  Adapter --> Dialog[Native chooser]
  Dialog --> Grant[Core maps the path to a shared directory]
  Grant --> Ref[Directory reference]
  Ref --> Connect[Existing connect folder command]
  Find --> Ref
```

The desktop shell only supplies a folder path from inside the application process. The web page never sends or receives a path; it receives a shared-directory reference like the browser flow. Core checks that the signed-in account is an administrator. It checks that the folder exists and can be read. If an existing grant contains the folder, it reuses that grant. Otherwise it adds a grant whose label is the folder's name and whose ID is derived from the path. At the next start, core restores the grant from the connected folder record. Grants do not exist in browser deployments because only the desktop shell can supply a path. Logs record only whether a folder was chosen or cancelled, never the path.

Limits: choosing a folder that contains an already connected folder from a different grant is not rejected, so the same files could be indexed twice until one folder is removed. Only one native chooser is open at a time. Closing the window while the chooser is open leaves it to the operating system.

Linux Docker tests cover the Music-folder lookup (override, fallback, missing, file and unreadable cases), the core grant mapping and restart, the HTTP routes and the desktop host with a fake chooser, and the web flow with the chooser stubbed. The real chooser on macOS, Windows and Linux (including GTK, window parenting and sandboxed paths) is device verification work and is not tested here.

## Removal and recovery

Removing a folder withdraws its sources and cancels its scans. Originals stay on disk. Recordings, playlists, likes, progress, and identity evidence remain. Another source can keep a recording playable. Reconnecting the same selection verifies files and restores the same source IDs when identity matches. Already-open streams retain their file descriptors; new requests recheck access.

Overlapping selections within a shared location are rejected. Setup rejects overlapping normalized host locations. Identical pathnames on different hosts have independent ownership. A device-local file stays unavailable on another host unless that host has another playable source.

Each scan attempt has its own token and the folder's current generation. Publication and reconciliation require both to remain valid. A recovered job cannot reuse an earlier attempt's observations. Incomplete enumeration preserves previously verified files. A failed root open withdraws only that folder's sources. Physically disconnected drives show unavailable folder status; their sources withdraw on a failed scan or revoked grant.

Opening and browsing have deadlines. Per-grant limits isolate stalled locations. Walking has a five-second inactivity deadline that observes unsupported entries, rather than a deadline for the entire scan. Abandoned calls retain their permit until they finish, with one operation thread and one walk thread per grant. Detached threads do not hold the async runtime open during shutdown. Operating-system calls cannot be forcibly cancelled in process.

Pending legacy witnesses are checked before ordinary matching. Incomplete legacy discovery fails conservatively rather than guessing identity. Metadata follows a surviving verified source when its previous owner becomes unavailable.

## Automatic rescans

The server watches every connected, bound server folder and queues a scan when its supported audio files are added, removed, renamed or changed. It uses the same scan job, lease, generation and attempt-token checks as a manual rescan. There is no second scanner.

```mermaid
flowchart LR
  Start[Watcher starts] --> Compare[Compare names, sizes and times with the last scan]
  Compare -- changed --> Quiet
  Change[File change] --> Signal[Native events or polling]
  Signal --> Quiet[Quiet period]
  Quiet --> Request{Active job?}
  Request -- none --> Queue[Queue scan]
  Request -- queued --> Wait[Wait for it]
  Request -- running --> Wait
  Wait -- finished --> Request
  Wait -- failed --> Retry[Queue again after a delay]
```

- Only audio and directory changes count. Native events for cover art, partial downloads (`.part`), hidden files such as `.DS_Store` and other non-audio files are ignored. A removed or renamed path counts unless its name is clearly noise (images, text and playlist files, `.part`, `.tmp`, `.DS_Store` and similar), because it may have been a directory such as `Dr.Dre` holding music. Renaming `track.part` to `track.mp3` counts. Polling compares only supported audio files, so it follows the same rule.
- A burst of changes collapses into one scan. The quiet period restarts on each change and is capped at ten times its length.
- A change during a running scan waits for that scan, then queues exactly one follow-up. A change while a scan is only queued adds nothing, because that scan has not started reading yet. Waiting uses plain reads with a short back-off, not write transactions.
- If a watched scan fails, Hito queues it again after one second, doubling the delay up to five attempts. Failed requests, such as a library with no administrator, back off from one to 30 seconds and log once.
- When a watcher starts (server start, reconnect), it compares file names, sizes and modification times with what the last scan indexed. It queues one scan only when a file was added, removed, resized or modified since that scan. It never reads file contents. A file copied in with an old modification time while the server was stopped is not found this way; run a manual rescan.
- Native operating-system events are the default. Hito polls instead when the operator asks for it, when the folder sits on a network, virtual or removable volume (detected on Linux), or when the system refuses another watch (for example at the inotify limit). Polling compares file names, sizes and modification times. Its walk never uses the scanner's walk slot, so a poll cannot make a scan fail. The wait grows to ten times the last walk when walks are slow. Polling and the start-up comparison read at most 500,000 audio files per folder and log when they stop at that cap.
- Each folder logs its backend choice once, with a reason code and no path.
- Watching never follows symbolic links out of the folder. Events outside the granted root are discarded. Events only trigger a scan; the scan itself still reads through the granted directory handle.
- Disconnecting or removing a folder stops its watcher. Reconnecting starts a new watcher for the new generation. A watcher that ends or fails while its folder is still connected is started again after a delay of one second, doubling up to a minute. Shutdown stops and joins every Hito watcher task before the server exits. The `notify` library's own event thread is only signalled to end when its watcher is dropped, and is not joined.
- Operators set the behaviour with `HITO_FOLDER_WATCH` (`auto`, `poll` or `off`) and `HITO_FOLDER_WATCH_POLL_SECONDS`. See [operations](operations.md#music-scanning).

Automated tests cover a new file, a removed file, a burst, a change during a running scan, a failed scan, ignored non-audio changes, a rename into audio, disconnect and reconnect, start-up comparison (changed, removed and unchanged), request back-off, the polling fallback, links leaving the folder, a disabled setting and shutdown. A unit test shows that a poll walk leaves the scanner's walk slot free. They ran on Linux inside Docker with temporary directories. Windows and macOS native watching, network volumes and removable drives are not yet tested.

## Evidence and limits

`./scripts/verify.sh` covers core/HTTP tests, generated contracts, browser tests, setup generation, Compose interpolation, formatting, Clippy, web build, and dependency audit. The browser flow adds two folders and checks desktop/mobile layout, status, Escape, focus restoration, playback, and account isolation.

Root-open fault injection uses the normal Linux backend. Enumeration injection explicitly builds rustix's libc backend so a libc shim can delay directory reads. It verifies source preservation, subsequent healthy-folder progress, shutdown, and successful traversal taking more than five seconds while unsupported entries continue. This does not prove production Linux raw-syscall outage handling or release latency targets.

Live native dialogs, permissions, drive reconnects, ambiguous-project recovery, rollback, signing, and packaging still need macOS, Windows, and Linux acceptance. PowerShell syntax was checked in Docker, which does not verify its Windows GUI. Automatic desktop Music-folder discovery and filesystem watching remain unfinished.
