import type { Schema } from "../shared/schema";
import { Link } from "@inertiajs/react";
import { Keyboard, Lock, LogOut } from "lucide-react";
import { useSignOut } from "../features/session/hooks/useSignOut";
import { useLockSession } from "../features/session/hooks/useLockSession";
import { useIdleLock } from "../features/session/hooks/useIdleLock";
import { usePlayer } from "../features/playback/components/PlaybackProvider";
import { clearStoredQueues } from "../features/playback/browser/QueueStorage";

import Avatar from "../shared/ui/Avatar";
import { accountName } from "../shared/accountName";
import { log } from "../shared/log";
import { currentDestination } from "./destinations";

export default function AccountMenu({
  user,
  lock,
  path,
  onShowShortcuts,
}: {
  user: Schema<"User">;
  lock: Schema<"LockStatus">;
  path: string;
  onShowShortcuts?: () => void;
}) {
  const player = usePlayer();
  const { busy, signOut } = useSignOut(user.id, () => {
    player.clear();
    clearStoredQueues();
  });
  const locking = useLockSession(user.id, () => player.clear());
  useIdleLock(user.id, lock, () => player.clear());
  return (
    <div className="account">
      <Link
        href="/settings"
        className="account-link"
        aria-label="Account and settings"
        aria-current={
          currentDestination(path) === "settings" ? "page" : undefined
        }
        onClick={() => log.info("navigation.requested", { route: "/settings" })}
      >
        <Avatar
          id={user.id}
          name={accountName(user)}
          version={user.avatar_version}
        />
        <span className="account-name">
          {accountName(user)}
          <small>{user.admin ? "Administrator" : "Account and settings"}</small>
        </span>
      </Link>
      {onShowShortcuts && (
        <button
          className="icon-button"
          aria-label="Keyboard shortcuts"
          aria-keyshortcuts="?"
          onClick={onShowShortcuts}
        >
          <Keyboard size={17} />
        </button>
      )}
      {lock.pin_enabled && (
        <button
          className="icon-button"
          aria-label="Lock"
          disabled={locking.busy}
          onClick={() => void locking.lock()}
        >
          <Lock size={17} />
        </button>
      )}
      <button
        className="icon-button"
        aria-label="Sign out"
        disabled={busy}
        onClick={() => void signOut()}
      >
        <LogOut size={17} />
      </button>
    </div>
  );
}
