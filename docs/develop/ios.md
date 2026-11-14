# iOS app

The iOS app lives in `apps/ios`. It's a SwiftUI app that connects to a Hito server. It doesn't keep a library on the phone.

What it does today:

- Opens on **Where is your music?**. A phone can't keep a library yet, so **On this phone** explains that and offers a server instead. **On a Hito server** opens **Connect to a server**. With no `http://` or `https://` typed, it tries `https` first, and `http` only when nothing answers there. A typed scheme is used exactly as typed.
- Shows **This connection is not encrypted** once per `http` server, before any password field.
- Asks to **Set up Hito** when the server has no accounts, and otherwise to **Sign in**.
- Keeps the session token in the Keychain and sends it as `Authorization: Bearer`. The server address and the last five servers are kept on the device.
- Shows **Can't reach your server** when the saved server doesn't answer at launch, and tries again every ten seconds.
- After **Sign out**, shows **Welcome back** with the remembered account. **Use another account** and **Forget me on this device** are on the same screen.
- When the account has a PIN, shows **Locked** once the server's idle time has passed, and asks for the PIN.

- Once signed in, shows **Home**, **Search**, **Library** and **Plugins**. Library has tracks (with a sort sheet), albums, artists and playlists. There are album and playlist pages, a mini player, **Now playing** with lyrics and the source list, and the queue. Likes sync to the server.
- Search can also ask one connected plugin that offers search, and adds or plays its results.
- **Plugins** shows what your plugins power and your connections. A plugin's page checks its connection, asks before accepting new permissions and disconnects it. Connecting a new plugin still happens on the web.
- **Settings**, from the account button on Home, has Profile (name and devices), Privacy & lock, Playback (source order) and About.
- Plays through the server's playback lease: it asks for the recording's sources in the listener's order, leases the first ready one and moves on to the next when one fails. Audio keeps playing with the screen locked. The app publishes the track to the system's now playing controls for the lock screen and headphones; those controls are not yet checked on a device.

Discover, connecting a plugin, changing your password and the administrator settings are not built yet. See [the tracking issue](https://github.com/acidiney/hitoapp/issues/187).

## Build and test

iOS can't build in Docker, so this part runs on a Mac with Xcode.

```sh
(cd apps/ios/HitoKit && swift test)
xcodebuild -project apps/ios/Hito.xcodeproj -scheme Hito -destination 'platform=iOS Simulator,name=iPhone 18 Pro' build
```

`HitoKit` holds the address, connection and account rules, and its tests use a fake server at the URL loading boundary. To try the app against a real server, run an isolated test server on a local port and connect the simulator to `localhost:<port>`. Never use your own server or library for this.
