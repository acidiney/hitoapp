# Your library

Your library is the music Hito knows about: files from music folders and tracks you saved through plugins. This page covers folders, browsing, search, editing track details and how Hito works out who the artists are.

## Add a music folder

Only the administrator can add folders. If you are a member and see no music, ask your administrator for access to a library.

1. Open **Settings → Music**.
2. Under **Music folders**, select **Add folder**.
3. Browse to a folder and select **Use this folder**.

The folder browser shows only locations that were shared with the server during setup. To add a folder on another drive, run the setup launcher again on the server computer and share that location. Then add it here. See [self-hosting](../self-hosting/README.md).

In the desktop app, **Choose folder…** opens your computer's own folder chooser. The desktop app may also offer "Add your Music folder?". Select **Use Music folder** to add it, or **Not now** to skip.

## Your files stay as they are

Folders are read-only. Hito reads the audio and its tags. It never moves, renames or retags your files. Edits you make in Hito stay in Hito.

## Scan a folder

Hito scans a folder as soon as you add it. It also watches connected folders and scans again when audio files are added, removed or changed.

To scan by hand, open **Settings → Music**. Select **Scan** next to a folder, or **Scan all** to scan every folder. A scan finds new files and changed tags.

Each folder shows its state:

- "Waiting to scan" or "Scanning · 120 files read" while it works.
- "Unavailable. Check that the drive is connected." when the drive is missing.
- "Scan failed. Check the folder and try again." when the scan could not finish.

## Remove a music folder

1. Open **Settings → Music**.
2. Open the menu next to the folder and select **Remove folder…**.
3. Select **Remove folder**.

Your files stay on disk. Playlists, likes and library records are kept. Music that exists only in that folder becomes unavailable.

Tracks whose files disappear are kept for 30 days, then removed. Tracks in someone's playlists, likes or downloads are kept. The administrator can clear them sooner. See [administration](administration.md#clear-out-missing-tracks).

## Browse your music

Open **Library** in the sidebar. The tabs across the top are **Tracks**, **Albums**, **Artists**, **Playlists**, **Podcasts**, **Audiobooks** and **History**. **Liked tracks** is in the sidebar.

Albums and artists come from the tags in your files and from tracks you saved through plugins.

To browse by genre, open **Discover**. **Genres you own** lists the busiest genres in your library. Genres come from your file tags. Tracks without a genre tag can get one from MusicBrainz when automatic artist lookups are on.

## Search

Open **Search** in the sidebar and start typing. Results update as you type and are grouped into tracks, albums and artists. Search ignores case and accents, so "bjork" finds "Björk".

Search covers your library first. If you connected plugins that can search, Hito can also search them. You choose this in [Privacy settings](#choose-what-hito-looks-up-online).

## Edit track details

You can change a track's title and artist. The change is private to your account. It does not touch the file or what other people see.

1. Open a track's menu and select **Track details**.
2. Open the **Metadata** tab.
3. Under **Details**, select the edit button next to **Title** or **Artist**.
4. Type the new value and select **Save personal title** or **Save personal artist**.

The edit follows the track everywhere in your account: albums, playlists, search and history. To go back to the file's value, select **Use source title** or **Use source artist**. **Reset all to source** undoes every edit on that track.

To change an album's title or album artist, open the album, open **More actions** and select **Edit album details**. Track artists stay as they are.

## How Hito identifies artists

Two files can say "Pedro" and mean different people. Hito does not treat equal names as the same artist. It looks for proof instead: MusicBrainz identifiers in your file tags, ISRC codes and artist links from plugins. When the proof is clear, Hito links the track to a MusicBrainz artist. Tracks linked to the same artist are grouped under one card in **Chosen artists** on the **Artists** page.

This runs in the background on the server. The **Artists** page shows how it is going, for example "Matching finished · 120 matched". Select **Match again** to run it now.

### Review artists Hito could not decide

When Hito finds a few possible artists but cannot prove which one is right, it asks you. On the **Artists** page, select the button that says how many artists need review.

- Select **Confirm** on the right artist. Hito applies it to that credit's tracks that have no artist yet.
- Select **None of these** if no candidate is right.
- Select **Search** on a credit marked "No match found automatically" to look it up yourself.

Suggestions you turned down stay under **Dismissed suggestions**. Select **Restore** to see one again.

### Choose the artist yourself

1. Open the track's **Track details** and go to the **Metadata** tab.
2. Under **Linked artist**, select **Change artist**, or **Choose the artist** if none is linked.
3. Select **Search artists**. Results show details such as the artist's type and country, so you can tell namesakes apart.
4. Select **Review artist** on the right result, then **Use this artist**.
5. Select **Choose this reviewed artist** to group the track under that artist.

Nothing changes until you confirm.

On an artist credit page, **Choose the artist** can apply one artist to every track with that credit. Hito shows what will change before it writes anything. Tracks that already have a different artist keep it.

### Undo an artist link

- To unlink one track, open its **Metadata** tab and select **Unlink** under **Saved artist reviews**. **Undo last artist review change** brings it back.
- If a chosen artist holds tracks by someone else, open that artist and select **Correct this artist**. Tick the tracks or albums, choose where they go and select **Move selected tracks**. **Undo last artist change** reverses the last move.

Your own choices always win over automatic matching. Moves do not change recordings, playlists, likes or your edits.

## Similar artists

A chosen artist's page can show **Similar artists**. This needs a plugin that offers similar artists, such as the ListenBrainz plugin, added to your metadata providers. Hito sends only the artist's MusicBrainz identity, never your listening history. Nothing is added to your library.

**Start radio** on a chosen artist plays tracks by that artist from your library, mixed with tracks by similar artists you also have.

## Set artist detail preferences

Open **Settings → Music → Artist details**.

- **Use metadata providers** turns artist profiles from providers on or off.
- MusicBrainz is built in and selected by default.
- Select **Add provider** to add a plugin you connected. You can select up to eight.
- Use **Move earlier** and **Move later** to set the order Hito asks them in.
- Select **Save metadata preferences** to keep your changes.

File tags and your own edits are always kept.

## Choose what Hito looks up online

Open **Settings → Privacy & lock**. The **What Hito sends out** card has three switches. Each is on until you turn it off.

- **Look up artists on MusicBrainz** sends track codes, artist links and sometimes artist names. Never album titles, file paths or audio.
- **Fetch album covers** asks the Cover Art Archive, using only a MusicBrainz identifier.
- **Search connected services** sends your search text to your plugins when your library has few matches.

The same card has **Load lyrics automatically**. That choice is saved in each browser, not with your account.

Turn off **Look up artists on MusicBrainz** and Hito stops background artist matching for your account. You can still choose artists by hand.
