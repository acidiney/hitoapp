import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { router } from "@inertiajs/react";

import Button from "../../../shared/ui/Button";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import AvatarSettings from "../../session/components/AvatarSettings";
import DisplayNameSettings from "../../session/components/DisplayNameSettings";
import OpenSessionsSettings from "../../session/components/OpenSessionsSettings";
import PasswordChangeSettings from "../../session/components/PasswordChangeSettings";
import { useSignOut } from "../../session/hooks/useSignOut";
import { setting } from "../model/settingsCatalog";

export default function ProfileSection({ user }: Schema<"SettingsPageProps">) {
  const player = usePlayer();
  const signOut = useSignOut(user.id, () => player.clear());
  const [passwordChanges, setPasswordChanges] = useState(0);
  const refreshUser = () => router.reload({ only: ["user"] });
  return (
    <>
      <SettingsCard setting={setting("account")}>
        <AvatarSettings
          key={`avatar:${user.id}`}
          user={user}
          onSaved={refreshUser}
        />
        <DisplayNameSettings
          key={`display-name:${user.id}`}
          account={user.id}
          username={user.username}
          displayName={user.display_name ?? null}
          onSaved={refreshUser}
        />
        <PasswordChangeSettings
          key={`password:${user.id}`}
          account={user.id}
          onChanged={() => setPasswordChanges((count) => count + 1)}
        />
        <SettingRow
          id="session"
          title="Sign out"
          hint="Ends the session on this device."
          action={
            <Button
              disabled={signOut.busy}
              onClick={() => void signOut.signOut()}
            >
              Sign out
            </Button>
          }
        />
      </SettingsCard>
      <OpenSessionsSettings
        key={`sessions:${user.id}`}
        account={user.id}
        version={passwordChanges}
      />
    </>
  );
}
