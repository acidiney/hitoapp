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

The library and the player are not built yet. See [the tracking issue](https://github.com/acidiney/hitoapp/issues/187).

## Build and test

iOS can't build in Docker, so this part runs on a Mac with Xcode.

```sh
(cd apps/ios/HitoKit && swift test)
xcodebuild -project apps/ios/Hito.xcodeproj -scheme Hito -destination 'platform=iOS Simulator,name=iPhone 18 Pro' build
```

`HitoKit` holds the address, connection and account rules, and its tests use a fake server at the URL loading boundary. To try the app against a real server, run an isolated test server on a local port and connect the simulator to `localhost:<port>`. Never use your own server or library for this.
