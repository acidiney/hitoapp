# Listening

This page covers playing music in the browser: the player, the queue, Now playing, downloads and Discover. To add music first, see [Library](library.md). To add sources, lyrics and more, see [Plugins](plugins.md).

## Play a list

Every list of tracks has **Play** and **Shuffle** buttons. This works on Tracks, Liked tracks, albums, artists, playlists and search results.

- **Play** queues the whole list, not only the rows you can see. The first tracks start at once and the rest load behind them.
- Click a row to play from that row to the end of the list.
- **Shuffle** plays the whole list in random order.

A queue holds up to 100,000 tracks. If a list is longer, Hito stops there and tells you.

## Use the player controls

The player sits at the bottom of every page. It has play and pause, previous, next, a seek bar, volume and mute.

- **Shuffle** turns random order on or off for the current queue.
- **Repeat** steps through three modes: off, repeat the whole queue, and repeat one track. A small 1 marks repeat one.
- **Open now playing** shows the large player.
- **Open queue** shows the queue.

## Add tracks to the queue

Open the menu on any track row. On a phone, press and hold the row.

- **Play next** puts the track right after the one playing. It appears under "Queued by you · plays first".
- **Add to queue** puts the track at the end.

If nothing is playing, both start the track straight away.

## Reorder the queue

Open the queue with **Open queue** or the Q key. It shows **Now playing**, then **Up next**. Tracks you already heard sit under **Played**.

- Drag a row by its handle to move it.
- Or open the row menu and choose **Move up** or **Move down**.
- From the keyboard, focus a row and press Alt with the up or down arrow. On a Mac, use Option.

Moving tracks never restarts the track that is playing.

## Remove tracks or clear the queue

- To remove one track, open its row menu and choose **Remove**.
- To empty the queue, select **Clear**, then **Clear queue** to confirm.

Clearing stops playback and empties the queue on this device. Your library and playlists do not change.

## Open Now playing

Select **Open now playing** in the player, or press N. On a phone, Now playing has three panels: **Lyrics**, **Up next** and **Source**. On a larger screen they sit side by side.

### Read lyrics

Lyrics need a lyrics plugin, such as LRCLIB. Without one, the panel shows "No lyrics plugin" and a link to connect one. See [Plugins](plugins.md).

- By default Hito looks up lyrics only when you select **Load lyrics**. To look them up for every track, turn on **Load lyrics automatically** in Settings, Privacy.
- Synced lyrics follow the song. The current line is highlighted. Select any line to jump to that point in the song.
- If you scroll by hand, following pauses. Select **Back to current line** to resume.
- Lyrics without timing show as plain text, marked "Not synced".
- If you have more than one lyrics plugin, select **Choose another provider** to compare.
- If none are found, select **Look up again**, or **Find lyrics** to search on the track's page.

### See and change the source

The **Source** panel shows where the current track is playing from: a music folder, a saved copy or a music service.

- **Change source** opens the track's page, where you can pick another source for this track. A source you choose for one track always wins over your general order.
- **Download** opens the track's page to save a copy. See below.

### Change the speed

**Speed** in the Source panel, and the speed menu in the player, change playback speed from 0.5× to 2×.

## Understand Ready and Not ready

Each track shows whether Hito can play it now.

- **Ready · Local** means the audio is in one of your music folders.
- **Ready · from YouTube**, or another service name, means a connected plugin can play it.
- **Ready · from your services** means a connected service can play it, but Hito has not checked which one yet.
- **No source** or **Not ready** means Hito has no playable audio for this track yet. Playlists copied from other services often start this way while Hito finds sources.

The queue skips tracks without a source. To find one, open the row menu and choose **Find audio**. For a track that plays the wrong audio, the same menu offers **Fix source**.

## When a source fails

A track can have several sources. Hito tries them in your source order, set in Settings, Playback, **Source order**. Local files and saved copies come first unless you move them.

If a source fails to open, Hito moves to the next ready source. On desktop, when a service is briefly unavailable, Hito looks for audio for the track again, up to three tries within about 15 seconds, before it skips the track. A service that refuses the request is not tried again. In the browser, Hito checks the services once more for new audio, then skips the track. You may see "An audio service is unavailable. Select the track again to retry."

To stop after the first failure instead, turn off **Try the next source if one fails** in the same settings.

## Download a single track

A track that plays from a music service can be saved as a copy on your Hito server.

1. Open the track's page and go to its **Sources** tab. **Download** in Now playing takes you there too.
2. Select **Download**. The page shows progress, then **Saved on this server**.

A saved copy plays without contacting the music service. It lives on the server, so the browser still needs a connection to it. To delete it, select **Remove download**.

If the server runs out of disk space, the page says so and Hito tries again later. If a download fails, remove it and select **Download** again.

## Use the keyboard and media keys

Press ? to see every shortcut. The main ones:

- Space plays or pauses.
- Shift with the right or left arrow goes to the next or previous track.
- M mutes, L likes the current track, Q opens the queue and N opens Now playing.
- G then H, L or P goes to Home, Library or Plugins.

Single-key shortcuts pause while you type in a field. You can turn them off in the shortcuts list.

Hito also shares the current track with the browser. Media keys, the system media panel and lock-screen controls show the title, artist and cover, and can play, pause, seek and skip. How much of this appears depends on your browser and device.

## Find music in Discover

Open **Discover** from the sidebar.

- **Start radio** builds a radio from an artist in your library. It starts with that artist's tracks. With a similar-artists plugin such as ListenBrainz connected, it mixes in nearby artists you also have. A radio adds nothing to your library.
- **Genres you own** lists the genres in your library. They come from your file tags, and from MusicBrainz for imported music. Select one to see its albums and tracks.
- **Search music** searches a connected music service outside your library. From the results you can play a track, queue it or add it to your library.

For podcasts and audiobooks, see [Podcasts and audiobooks](podcasts-and-audiobooks.md). For the desktop app and installing Hito on a phone, see [Apps](apps.md).
