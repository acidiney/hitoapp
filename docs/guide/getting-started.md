# Getting started

This page takes you from a fresh Hito server to music playing. It assumes the server is already running. If it is not, start with [self-hosting](../self-hosting/README.md).

Hito is in active development and has no public release yet. These steps describe the current development build.

## Open Hito

Open Hito in your browser. With the setup launcher the address is usually `http://localhost:8080`. If someone else runs the server, ask them for the address.

## Create the first account

A new server has no accounts. Hito shows **Set up Hito** instead of a sign-in page.

1. Enter a **Username**.
2. Enter a **Password** of at least 12 characters. A long phrase is easier to remember.
3. Select **Create administrator**.

The first account is the administrator. It can add music folders, create accounts for other people and offer plugins to the server. Everyone you add later is a member. See [administration](administration.md).

Keep this password safe. Hito has no "forgot password" email.

## Add a music folder

Hito reads music from folders that were shared with the server when it was set up. It never moves or changes your files.

1. Open **Settings → Music**.
2. Under **Music folders**, select **Add folder**.
3. Browse to the folder you want and select **Use this folder**.

Hito starts scanning right away. The folder row shows progress, for example "Scanning · 120 files read". Albums and artists appear as the scan finds them.

If the folder you want is not in the list, it has not been shared with the server yet. Run the setup launcher again on the server computer and choose that folder. [Your library](library.md) has more on folders and scanning.

You can skip this step. Hito also works with plugins alone.

## Connect a plugin

Plugins add more places to find music, playlists from other services, lyrics and covers. All of them are optional.

1. Open **Plugins** in the sidebar.
2. Under **Available on this server**, select **Connect** next to the service you want.
3. Select **Continue**, read what the plugin declares, then select **Accept and connect**.

If nothing is available, the server offers no plugins yet. As the administrator, you can offer one under **This server**. See [plugins](plugins.md) and [administration](administration.md).

## Play something

1. Open **Library** in the sidebar.
2. Pick **Tracks**, **Albums** or **Artists** at the top.
3. Select a track to play it, or open an album and select **Play**.

Music keeps playing while you move around the app. [Listening](listening.md) covers the player, the queue and lyrics.

When the library is empty, **Home** shows shortcuts: **Choose a music folder**, **Import a playlist** and **Connect a service**.

## Install Hito on your phone

Open the same address in your phone's browser and add Hito to your home screen. It then opens like an app. [Apps](apps.md) has the steps for each phone and for the desktop app.

## Next steps

- Make the account yours: set a display name, a photo and a screen lock PIN. See [your account](account.md).
- Build playlists or import one from a link. See [playlists](playlists.md).
- Subscribe to podcasts or add audiobooks. See [podcasts and audiobooks](podcasts-and-audiobooks.md).
- Add accounts for the people you live with. See [administration](administration.md).
