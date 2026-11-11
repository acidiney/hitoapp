import { useEffect, useRef, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { accountName } from "../../../shared/accountName";
import ActionMenu from "../../../shared/ui/ActionMenu";
import Avatar from "../../../shared/ui/Avatar";
import Button from "../../../shared/ui/Button";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { accountHint } from "../model/accountText";
import { useAccounts } from "../hooks/useAccounts";
import AccountDialog from "./AccountDialog";
import DeleteAccountDialog from "./DeleteAccountDialog";
import { setting } from "../../settings/model/settingsCatalog";

export default function AccountSettings({
  account,
  library,
  libraryName = "this library",
}: {
  account: string;
  library?: string;
  libraryName?: string;
}) {
  const { users, pending, create, remove, access } = useAccounts(account);
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<Schema<"User"> | null>(null);
  const opener = useRef<HTMLButtonElement>(null);
  const wasCreating = useRef(false);
  useEffect(() => {
    if (wasCreating.current && !creating)
      opener.current?.focus({ preventScroll: true });
    wasCreating.current = creating;
  }, [creating]);
  return (
    <SettingsCard
      setting={setting("accounts-access")}
      description="Each account has its own playlists, likes and listening history."
      action={
        <Button ref={opener} onClick={() => setCreating(true)}>
          Create account
        </Button>
      }
    >
      {users.map((user) => (
        <SettingRow
          key={user.id}
          title={accountName(user)}
          leading={
            <Avatar
              id={user.id}
              name={accountName(user)}
              version={user.avatar_version}
            />
          }
          hint={accountHint(user)}
          action={
            user.id === account ? (
              <span className="setting-line-note">You</span>
            ) : (
              <ActionMenu
                label={`More for ${accountName(user)}`}
                triggerClassName="more-button"
                align="end"
                items={[
                  ...(!user.admin && library
                    ? [
                        {
                          label: `Give access to ${libraryName}`,
                          onSelect: () => void access(library, user, true),
                        },
                        {
                          label: `Remove access to ${libraryName}`,
                          onSelect: () => void access(library, user, false),
                        },
                      ]
                    : []),
                  {
                    label: "Delete account…",
                    destructive: true,
                    onSelect: () => setDeleting(user),
                  },
                ]}
              />
            )
          }
        />
      ))}
      {creating && (
        <AccountDialog
          pending={pending}
          onCreate={create}
          onClose={() => setCreating(false)}
        />
      )}
      {deleting && (
        <DeleteAccountDialog
          user={deleting}
          pending={pending}
          onDelete={remove}
          onClose={() => setDeleting(null)}
        />
      )}
    </SettingsCard>
  );
}
