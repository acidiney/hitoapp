# Apps

You can use Hito in three ways: in any browser, as a web app installed from the browser, or in the desktop app. The desktop app is still in development.

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
- The web app needs a connection to your server to play music. If it cannot reach the server, it shows "Hito can't reach your server" with a **Retry** button. Your place is kept.
- Downloads are saved on your server, not on the phone. Offline listening on the phone is not available in the web app.
- Lock-screen behavior, background audio and track changes while the screen is off depend on the browser and phone. They have not been checked on every device.

## The desktop app

The desktop app runs Hito on your computer without a server. It is in development and has no installers yet.

### What it is

- **Its own private library.** The desktop app keeps its own Hito library on your computer. It does not need a separate Hito server, Docker or an internet connection for your own music.
- **Your Music folder.** It can read the system Music folder after you confirm it in Settings. Nothing is scanned before you confirm.
- **Native playback.** Audio plays through Hito's own player on your computer, not through a browser. The screens are the same as the web app.

### What is not available yet

- There are no installers or packages. Building it today needs the developer setup.
- Shuffle, repeat, previous track and playback speed are not available in the desktop player yet.
- Operating-system media controls and the lock screen do not show the desktop app's track yet.
- Connecting the desktop app to a Hito server, with playlists and progress that sync, is planned.
- Offline downloads on all platforms are planned.

The [roadmap](../roadmap.md) lists what the desktop app still needs before the first release, including signed installers for macOS, Windows and Linux.

## Which one to use

- To listen on a phone, install the web app.
- To share music with family or listen on several devices, run a server and use the browser or web app. See [Getting started](getting-started.md).
- To try Hito on one computer without a server, the desktop app will be the way once it has installers. Until then, use the server with Docker.

Accounts, sign-in and the PIN lock work the same in each. See [Account](account.md).
