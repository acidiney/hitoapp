import { Link } from "@inertiajs/react";
import { Lock, LogOut } from "lucide-react";
import { useSignOut } from "../features/session/hooks/useSignOut";
import { useLockSession } from "../features/session/hooks/useLockSession";
import { useIdleLock } from "../features/session/hooks/useIdleLock";
import { usePlayer } from "../features/playback/components/PlaybackProvider";
import type { LockStatus, User } from "../shared/types";
import { log } from "../shared/log";
import { currentDestination } from "./destinations";

export default function AccountMenu({
  user,
  lock,
  path,
}: {
  user: User;
  lock: LockStatus;
  path: string;
}) {
  const player = usePlayer();
  const { busy, signOut } = useSignOut(user.id, () => player.clear());
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
        <span className="avatar">{user.username[0]?.toUpperCase()}</span>
        <span>
          {user.username}
          <small>{user.admin ? "Administrator" : "Account and settings"}</small>
        </span>
      </Link>
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
