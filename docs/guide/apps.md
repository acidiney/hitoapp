# Apps

You can use Hito in three ways: in any browser, as a web app installed from the browser, or in the desktop app. The desktop app is still in development. An iPhone app is also in development.

## Install Hito as a web app

Installing puts Hito on your home screen, dock or app list. It opens in its own window, without the browser's address bar. It is the same Hito you use in the browser, signed in to the same server.

Before you start, open your Hito server in the browser and sign in. Browsers usually offer to install a web app only when the server is reached over HTTPS or as localhost. See [Administration](administration.md) for how your server is reached.

### On an iPhone or iPad

1. Open Hito in Safari.
2. Tap the Share button.
3. Tap **Add to Home Screen**, then **Add**.

Hito appears on your home screen with its own icon.

### On Android

1. Open Hito in Chrome.
2. Open the menu with the three dots.
3. Tap **Install app** or **Add to Home screen**, then confirm.

### On a computer

- In Chrome or Edge, select the install icon at the right of the address bar, or open the menu and choose the install option for Hito.
- In Safari on a Mac, choose **File**, then **Add to Dock**.

The exact menu names change between browser versions.

## What works in the web app

Everything you can do in the browser works in the installed web app: your library, playlists, Discover, podcasts, audiobooks, plugins and settings. Hito adjusts its layout for phones, with Now playing split into Lyrics, Up next and Source panels.

- The current track shows in the system media controls and, where the phone allows it, on the lock screen. See [Listening](listening.md).
- The web app needs a connection to your server to play music. If it cannot reach the server, it shows "Cannot reach Hito" with a **Try now** button. Your place is kept.
- Downloads are saved on your server, not on the phone. Offline listening on the phone is not available in the web app.
- Lock-screen behavior, background audio and track changes while the screen is off depend on the browser and phone. They have not been checked on every device.

## The desktop app

The desktop app opens either its own library on your computer or a Hito server. It is in development and has no installers yet.

### Choosing where your music is

The first time it opens, the app asks **Where is your music?**

- **On this computer** starts a private library on this computer for your own folders. Other devices can't reach it.
- **On a Hito server** asks for the server's address, the one you open in a browser. You can leave out `http://` or `https://`. If the server uses plain http, the app warns once that the connection is not encrypted. Then sign in, or create the administrator if the server has no accounts yet.

The app remembers the choice and the last five servers. To change it, open **Settings → About** and select **Change…** next to **This app uses**. Your library on this computer is kept, and you stay signed in to the server.

If the app can't reach your server when it opens, it shows **Can't reach your server** and tries again every ten seconds. **Change server** goes back to the first screen.

Your sign-in for each server is kept in the computer's own password store: Keychain on macOS, Credential Manager on Windows and the Secret Service on Linux.

### What it is

- **Its own private library.** On this computer, the desktop app keeps its own Hito library. It does not need a separate Hito server, Docker or an internet connection for your own music.
- **Your Music folder.** It can read the system Music folder after you confirm it in Settings. Nothing is scanned before you confirm.
- **Native playback.** Audio plays through Hito's own player, not through a browser, both for the library on this computer and for a Hito server. Audio the player can't read directly, such as Opus, is converted on this computer with FFmpeg as it plays, from either library. The screens are the same as the web app.

### What is not available yet

- There are no installers or packages. Building it today needs the developer setup.
- **Streaming quality** is not offered in the desktop app's library on this computer. Its own files play at their original quality.
- Offline downloads on all platforms are planned.

The [roadmap](../roadmap.md) lists what the desktop app still needs before the first release, including signed installers for macOS, Windows and Linux.

## The iPhone app

The iPhone app connects to a Hito server and plays through it. It keeps no library on the phone. It is in development and is not in the App Store. Building it needs a Mac with Xcode; see [the iOS app](../develop/ios.md).

In the iPhone app, touch and hold a track, album, playlist, artist or Home shortcut to see its actions. They are the same actions as its **More actions** menu.

## Which one to use

- To listen on a phone, install the web app.
- To share music with family or listen on several devices, run a server and use the browser or web app. See [Getting started](getting-started.md).
- To try Hito on one computer without a server, the desktop app will be the way once it has installers. Until then, use the server with Docker.

Accounts, sign-in and the PIN lock work the same in each. See [Account](account.md).
