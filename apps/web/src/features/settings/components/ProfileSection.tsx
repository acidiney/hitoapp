import type { Schema } from "../../../shared/schema";
import { LogOut, Lock } from "lucide-react";

import Button from "../../../shared/ui/Button";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import { useLockSession } from "../../session/hooks/useLockSession";
import AccountSecurity from "../../session/components/AccountSecurity";
import { useSignOut } from "../../session/hooks/useSignOut";

export default function ProfileSection({
  user,
  lock,
}: Schema<"SettingsPageProps">) {
  const player = usePlayer();
  const signOut = useSignOut(user.id, () => player.clear());
  const locking = useLockSession(user.id, () => player.clear());
  return (
    <>
      <SettingsCard title="Account">
        <div className="profile-identity">
          <span className="avatar large" aria-hidden="true">
            {user.username[0]?.toUpperCase()}
          </span>
          <span className="setting-line-copy">
            <span className="setting-line-title">{user.username}</span>
            <span className="setting-line-hint">
              {user.admin ? "Administrator" : "Member"}
            </span>
          </span>
        </div>
      </SettingsCard>
      <SettingsCard title="Session">
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
