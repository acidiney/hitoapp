# Playlists

You can make your own playlists, like tracks, and bring in playlists from other services through a plugin. Playlists are private to your account.

## Make a playlist

1. Open **Playlists** and select **New playlist**.
2. Give it a name and select **Create playlist**.

To add a track, open the track's menu and select **Add to playlist**, then pick the playlist. You can add the same track more than once.

To change a playlist you made, open it and select **Edit**. You can rename it, move tracks up or down and remove tracks. Select **Done editing** to save, or **Cancel** to go back.

To delete it, select **More options**, then **Delete playlist**. Deleting a playlist never deletes the music in it.

## Like a track

Select **Like** on a track, in a track row or on the track's page. Liked tracks are collected in **Liked tracks** in the sidebar. Select **Unlike** to remove a track from it.

## Bring in a playlist from a link

You need a connected plugin that offers playlists. Hito ships with one for public Spotify playlists. See [plugins](plugins.md).

1. Open **Playlists** and select **Import from a link**. You can also paste the link into search, then select **Open** on the playlist link hint.
2. Paste the link in **Playlist link or identifier**. Hito picks the matching service. If you have several and it can't tell, choose one in **Playlist service**.
3. Under **How to add it**, choose **Follow** or **Copy**.
4. For Follow, choose how often to **Check for changes**: **Every hour** or **Every day**.
5. Select **Follow playlist** or **Import a copy**.

Hito copies the track list and order. It doesn't copy audio. The playlist must be public, or the service will refuse it.

### Copy or follow

| | Copy | Follow |
| --- | --- | --- |
| Can you edit it? | Yes. It's yours. | No. It's read-only. |
| Does it change when the service does? | No. | Yes. Hito checks every hour or every day. |
| Shown as | "Your playlist · a copy of ..." | "Followed from ... · read-only" |

You can do both with the same link. Each becomes its own playlist.

## Keep a followed playlist up to date

A followed playlist shows **Followed playlist.** with when it was last checked and when the next check is due.

- Select **Refresh now** to check right away.
- If an update fails, the playlist keeps its last good tracks. Hito tries again at the next check.
- To stop following, delete the playlist. Hito stops checking it.

Your likes and listening progress on its tracks stay the same when the playlist changes.

### When updates pause

If the plugin behind a followed playlist changes in a way that matters, for example it asks for a new capability or its token changes, Hito pauses updates. The playlist page explains what changed. Your saved tracks stay available.

- Select **Resume updates** to accept the change and continue.
- Select **Review connection** to look at the plugin in Settings first.

If the plugin was removed or no longer offers playlists, updates can't resume. Small changes, such as a plugin restart with nothing new, don't pause anything.

## Tracks that aren't ready yet

A playlist from another service often lists tracks you have no audio for yet. The playlist header shows how many are ready, for example "43 ready · 7 not ready yet".

- **Play 43 ready** plays only the tracks that can play now.
- **Ready** and **Not ready** filter the list.
- In a copy, **Edit** offers **Remove the 7 not ready**.

A track becomes ready when Hito has a source for it. That can be a file in your music folders, a saved copy, or a plugin that offers playback. When you play a track that isn't ready, Hito looks for audio through your connected plugins. You can also select **Find audio** in the track's menu. If no plugin offers playback, the tracks stay not ready. [Find audio for tracks you don't have](plugins.md#find-audio-for-tracks-you-dont-have) explains how matching works.

## Download a playlist to the server

Downloading saves copies of the audio on the Hito server. Saved copies keep playing when the plugin they came from is offline or removed.

1. Open the playlist and select **Download**.
2. The **Playlist download** section shows how many entries are saved.

Some things to know:

- Copies are saved on the server, not in your browser. The browser still needs a connection to the server to play them. The desktop app saves copies on that device instead.
- Entries without a source wait. The section says how many are waiting for a source.
- If the server runs out of disk space, Hito waits and tries again by itself.
- If some entries fail, select **Retry failed downloads**.
- Some plugins only allow playing, not saving. Hito won't save copies from them. The plugin's review screen tells you which it is.

Playing, following or importing a playlist never downloads anything by itself.

To remove the copies, open the playlist and select **Remove download**. Hito deletes the saved copies unless another download still uses them. Your music folders, playlists and likes are not touched.

## Download one track

1. Open the track and go to its **Sources** tab.
2. Under **Server copy**, select **Download**.

When it's done, the track shows **Saved on this server**. Select **Remove download** to delete the copy.

If a download failed, remove it, then select **Download** again. If you changed the track's source after downloading, Hito tells you the copy belongs to a previous source and offers **Download selected source**.

## Find missing albums in a copy

Some services don't include album details in a playlist. For an imported copy, the playlist page has a **Playlist metadata** section.

Select **Find missing albums**. Hito asks the plugin you imported from for album details of tracks that have none. It doesn't replace albums that are already set, and it doesn't add new tracks from the service. Your edits, order, likes and source choices stay.

If the lookup can't be confirmed, select **Retry album lookup**.
