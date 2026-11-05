import type { Schema } from "../../../shared/schema";
import { router } from "@inertiajs/react";
import { LogOut, Lock } from "lucide-react";

import { accountName } from "../../../shared/accountName";
import Avatar from "../../../shared/ui/Avatar";
import Button from "../../../shared/ui/Button";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import { useLockSession } from "../../session/hooks/useLockSession";
import AccountSecurity from "../../session/components/AccountSecurity";
import DisplayNameSettings from "../../session/components/DisplayNameSettings";
import { useSignOut } from "../../session/hooks/useSignOut";
import { setting } from "../model/settingsCatalog";

export default function ProfileSection({
  user,
  lock,
}: Schema<"SettingsPageProps">) {
  const player = usePlayer();
  const signOut = useSignOut(user.id, () => player.clear());
  const locking = useLockSession(user.id, () => player.clear());
  return (
    <>
      <SettingsCard setting={setting("account")}>
        <div className="profile-identity">
          <Avatar id={user.id} name={accountName(user)} large />
          <span className="setting-line-copy">
            <span className="setting-line-title">{accountName(user)}</span>
            <span className="setting-line-hint">
              {user.display_name && `${user.username} · `}
              {user.admin ? "Administrator" : "Member"}
            </span>
          </span>
        </div>
      </SettingsCard>
      <DisplayNameSettings
        key={`display-name:${user.id}`}
        account={user.id}
        username={user.username}
        displayName={user.display_name ?? null}
        onSaved={() => router.reload({ only: ["user"] })}
      />
      <SettingsCard setting={setting("session")}>
        {lock.pin_enabled && (
          <SettingRow
            title="Lock this session"
            hint="Asks for your PIN before anyone can continue."
            action={
              <Button
                disabled={locking.busy}
                onClick={() => void locking.lock()}
              >
                <Lock size={16} aria-hidden="true" />
                Lock now
              </Button>
            }
          />
        )}
        <SettingRow
          title="Sign out"
          hint="Ends the session on this device."
          action={
            <Button
              variant="destructive"
              disabled={signOut.busy}
              onClick={() => void signOut.signOut()}
            >
              <LogOut size={16} aria-hidden="true" />
              Sign out
            </Button>
          }
        />
      </SettingsCard>
      <AccountSecurity key={`security:${user.id}`} account={user.id} />
    </>
  );
}
