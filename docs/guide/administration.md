# Administration

This page is for the administrator: the person who created the first account on the server. It covers accounts, library access, music folders and plugins. To install or update the server itself, see [self-hosting](../self-hosting/README.md).

Members do not see these controls.

## Create an account

1. Open **Settings → Accounts**.
2. Under **People**, select **Create account**.
3. Enter a **Username** and a **Password** of at least 12 characters.
4. Select **Create account**.

Give the person their username and password. They can change the password after they sign in. See [your account](account.md).

New accounts are members. Each one has its own playlists, likes and listening history.

## Give someone access to the library

A new member sees no music until you give them access.

1. Open **Settings → Accounts**.
2. Open the menu next to the person.
3. Select **Give access to** followed by the library name.

To take access away, select **Remove access to** in the same menu. Their playlists and likes stay in their account, but they can no longer browse or play that library.

Administrators always have access, so the menu shows these options only for members.

## Add music folders for the server

Music folders belong to the server and are shared with everyone who has access to the library.

1. Open **Settings → Music**.
2. Under **Music folders**, select **Add folder**.
3. Browse to a folder and select **Use this folder**.

You can only pick locations that were shared with the server during setup. To add a folder on another drive, run the setup launcher on the server computer, share that location, then add it here.

If the server has more than one library, pick one under **Library** first.

Hito scans new folders right away and rescans when files change. **Scan** and **Scan all** start a scan by hand. [Your library](library.md#scan-a-folder) explains the folder states.

## Remove a music folder

1. Open **Settings → Music**.
2. Open the menu next to the folder and select **Remove folder…**.
3. Select **Remove folder**.

The files stay on disk. Playlists, likes and library records are kept. Music that exists only in that folder becomes unavailable for everyone.

## Clear out missing tracks

When files disappear, Hito keeps their tracks for 30 days before removing them. Tracks that anyone has in a playlist, liked, with progress, downloaded or edited are always kept.

To remove them now:

1. Open **Settings → Music**.
2. Next to **Tracks with missing files**, select **Remove now…**.
3. Confirm in the **Remove missing tracks now?** dialog.

Your files are not touched. Every music folder must be connected and finished scanning, or Hito removes nothing, so an unplugged drive never costs you tracks. If many tracks are missing, Hito asks you to remove again to continue.

## Offer a plugin to the server

Plugins run as separate services. An offer makes a plugin available to every account on the server. It does not connect anyone. Each person still chooses for themselves.

1. Open **Plugins** in the sidebar.
2. Select the **This server** tab.
3. Under **Offer another plugin**, enter the **Service address**.
4. Select **Offer service**.

Hito contacts the address from the server and reads the plugin's name and what it can do. Some services need a token before they describe themselves. For those, open **Advanced settings** and enter a **Custom label (optional)**.

If the server was started with the reference plugins, some offers may already be listed. See [self-hosting](../self-hosting/README.md) for how to start them.

People then connect the plugin under **Your plugins**. See [plugins](plugins.md).

## Withdraw a plugin

1. Open **Plugins** in the sidebar and select **This server**.
2. Select **Withdraw** next to the service.
3. Confirm with **Withdraw**.

Everyone on the server is disconnected from it and their saved tokens are cleared. Libraries, playlists and saved copies stay.

You cannot see or change other people's plugin connections or tokens.

## Correct an artist for everyone

On a chosen artist's page, **Correct this artist** moves tracks that belong to someone else. As the administrator you can also select **Apply this correction for everyone**. Every account then sees the fix.

To review or undo these, open **Settings → Accounts → Shared artist corrections** and select **Withdraw**. Accounts that chose an artist themselves keep their own choice.

## See who is active

**Settings → Accounts** lists each person with their role and when they were last active, for example "Member · Last active 5 minutes ago". The time can be a few minutes behind. You do not see their devices, playlists or history.

## Delete an account

1. Open **Settings → Accounts**.
2. Open the menu next to the person and select **Delete account…**.
3. Type their username to confirm.
4. Select **Delete account**.

This signs them out everywhere and permanently erases their likes, listening history, playlists, personal edits, plugin connections, downloads and settings. Shared library files stay. It cannot be undone.

You cannot delete your own account from this list.

## Things you cannot do yet

- Reset another person's password.
- Change a member into an administrator.
- Check for updates from inside Hito. Install a newer build on the server to update.
