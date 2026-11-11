import type { Schema } from "../shared/schema";
import { Link } from "@inertiajs/react";
import { SlidersHorizontal } from "lucide-react";
import { useSignOut } from "../features/session/hooks/useSignOut";
import { useLockSession } from "../features/session/hooks/useLockSession";
import { useIdleLock } from "../features/session/hooks/useIdleLock";
import { usePlayer } from "../features/playback/components/PlaybackProvider";
import { clearStoredQueues } from "../features/playback/browser/QueueStorage";

import ActionMenu, { type ActionMenuItem } from "../shared/ui/ActionMenu";
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
  const options: ActionMenuItem[] = [
    ...(onShowShortcuts
      ? [{ label: "Keyboard shortcuts", onSelect: onShowShortcuts }]
      : []),
    ...(lock.pin_enabled
      ? [
          {
            label: "Lock",
            disabled: locking.busy,
            onSelect: () => void locking.lock(),
          },
        ]
      : []),
    { label: "Sign out", disabled: busy, onSelect: () => void signOut() },
  ];
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
      <ActionMenu
        label="Account options"
        triggerClassName="icon-button"
        icon={<SlidersHorizontal size={17} aria-hidden="true" />}
        align="end"
        items={options}
      />
    </div>
  );
}
