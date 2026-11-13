# Plugins

A plugin adds something Hito can't do on its own: more places to find and play music, playlists from other services, track details, covers or lyrics. Plugins are optional. Hito plays the music in your folders without any.

Each plugin is a separate service that runs next to your Hito server. Hito never downloads or runs plugin code. It only sends the plugin the requests it needs. Any plugin that follows the plugin protocol works the same way, so you can use the ones that ship with Hito, someone else's, your own, or none.

To write your own, see [write a plugin](../develop/plugins.md).

## What a plugin can do

When you connect a plugin, Hito shows which of these it offers:

| Capability | What it does for you |
| --- | --- |
| Search | Find music beyond your library, in Search and Discover. |
| Playback | Play tracks you have no file for. |
| Playlists | Follow or copy a playlist from a link. |
| Track details | Fill in missing titles, artists and albums. |
| Covers | Fetch album artwork. |
| Lyrics | Show lyrics in Now playing. |

A plugin only does what you approved. If it later wants more, it asks first.

## Connect a plugin

An administrator first offers the plugin to the server. After that, each person connects it to their own account. Connections are private. Nobody else, including the administrator, can see or use yours.

1. Open **Settings → Plugins**. The **Your plugins** tab lists what you have.
2. Under **Available on this server**, select **Connect** next to the plugin, for example **Connect YouTube**.
3. If the plugin's operator gave you a token, paste it in **Bearer token (optional)**. Most plugins need none. The token stays private to your account.
4. Select **Continue**. Hito checks the service.
5. **Before you connect** lists what the plugin will be able to do and whether it allows saving audio. Read it.
6. Select **Accept and connect**, or **Back** to leave without connecting.

The plugin now appears under **Your connections** with a short description and a status such as "Connected and answering".

If nothing is listed under **Available on this server**, no plugin has been offered yet. An administrator can offer one from the **This server** tab. See [administration](administration.md). Starting plugin services on the server is covered in [self-hosting](../self-hosting/README.md).

## Plugins that ship with Hito

These are reference plugins. They are examples of what a plugin can do. None is required, and Hito works the same with plugins from anyone else.

| Plugin | What it adds | What it needs |
| --- | --- | --- |
| Spotify public playlists | Copy or follow a public Spotify playlist. Titles, artists, albums and order only. It plays no audio. | Nothing. No Spotify account. |
| YouTube | Search, playback, track details and covers. | Nothing. See the limit below. |
| LRCLIB | Lyrics. | A token from whoever runs the plugin. |
| Jamendo | Search and play free-licensed music. | A Jamendo client ID that the server operator sets up once. |
| ListenBrainz | A "Similar artists" section on artist pages. | Server setup only. No ListenBrainz account. |
| Real-Debrid, All-Debrid, TorBox, Premiumize | Play files through a debrid account. | The operator's debrid account and a list of files to offer. |
| Folder | Serves a folder of music, or three demo tones, as a plugin source. Useful for testing. | Nothing. |

Some known limits:

- **YouTube** may refuse to hand over audio when the plugin runs on a hosted server or VPS. Search and track details still work. Hito then plays from your other sources, or tells you no source was found. On a home connection, audio has worked in tests, but YouTube can change this at any time.
- **Debrid plugins** play only the recordings the operator lists for them. They don't search your debrid account.
- **Spotify** reads public playlist pages. Private playlists can't be imported.

## Choose which source plays first

A track can have several sources: a file in your music folders, a copy saved on the server, and one or more plugins. **Playback order** on the Plugins page decides which one Hito tries first.

1. Open **Settings → Plugins**.
2. In **Playback order**, use the up and down arrows to move **Local files**, **Saved copies** and each plugin.
3. Select **Save playback order**.

Local files and saved copies start at the top. A plugin you connect later goes to the bottom until you move it.

Leave **Try the next source if one fails** on to have Hito move to the next source when one doesn't open. It tries up to six sources for one play. Turn it off to stop at the first failure.

A source you choose by hand for one track always wins over this order.

## Find audio for tracks you don't have

When you play a track that has no ready source, Hito looks for one through your connected plugins that offer search and playback. It asks up to four of them, in your playback order. It compares title, artist, length and version, such as live or remix, before it accepts a match.

If no plugin has an exact match, Hito may pick the plugin's top result and mark the source **Best guess**. Check it, and fix it if it's wrong.

To look by hand, open the track's menu and select **Find audio**. On the track's **Sources** tab, **Find a source** lets you search your plugins and review the match before Hito adds it.

Loading a page never starts a search. Hito only looks when you play a track or ask it to.

## Choose a source by hand

1. In Now playing, select **Change source**. You can also open the track's **Sources** tab, or select **Fix source** from its menu.
2. Under **Playback source**, pick the source you want.
3. Select **Use source**.

Hito keeps playing from that source for this track, even when other sources exist. To let Hito choose again, select **Return to automatic**.

## Sources that need preparation

Some plugins can't play a file right away. The service behind them has to get it ready first. For these, the track's **Sources** tab shows **Audio availability**:

1. Select **Check availability**.
2. If the audio isn't ready, select **Prepare audio**.
3. Hito shows the progress. When preparation finishes, you can play the track.

If preparation is blocked, Hito says why, for example that the service's quota is reached or that you need to reconnect.

Preparing audio doesn't save a copy. To keep one on the server, use **Download**. See [playlists](playlists.md).

## When a plugin changes

A plugin can change what it offers after an update. When you open **Settings → Plugins**, a notice such as "New: album covers" appears on its card. The new capability stays off until you select **Review** and accept it. Nothing else changes in the meantime.

To check a plugin yourself, select **Manage** on its card, then **Check connection**.

## When a plugin asks you to sign in again

Some plugins use your account on another service. Their card has an **Account sign-in** line.

- **Connect** opens the service's sign-in page in a new tab. Finish there. The Plugins page updates by itself.
- **Sign in again needed** means the service no longer accepts the saved sign-in. Select **Reconnect** and sign in again.
- **Disconnect** deletes the saved sign-in from Hito. To revoke Hito's access on the service itself, do that on the service's own site.

None of the reference plugins need an account sign-in today.

## Change or remove a token

Select **Manage** on the plugin's card. Under **Connection details**, enter a **New bearer token** and select **Save token**. Hito checks it before saving. **Clear saved token** removes it.

## Disconnect a plugin

1. Select **Manage** on the plugin's card.
2. Select **Remove**, then confirm.

Hito clears the saved token. Your library, playlists and saved copies stay. Tracks that only play from that plugin show **No source** until another source is found. You can connect the plugin again later.

## What plugins can and can't see

A plugin sees only what Hito sends it for a task:

- the words you search for,
- the title and artist of a track Hito is finding audio for,
- the playlist link you import or follow,
- the artist Hito asks about for similar artists, never your listening history,
- the token you entered for that plugin, if any.

A plugin can't see your library, your music files, your account name, your other plugins or your browser. Your browser never talks to a plugin directly. Audio and covers reach you through Hito.

The services behind a plugin, such as YouTube or Spotify, see requests coming from your server's internet address.

Tokens you enter are encrypted on the server and private to your account.
