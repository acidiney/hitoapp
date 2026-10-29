import type { SettingsPageProps } from "../../../shared/types";
import AccountSettings from "../../accounts/components/AccountSettings";
import SharedCorrectionsSettings from "../../artists/components/SharedCorrectionsSettings";

export default function AccountsSection({
  user,
  libraries,
}: SettingsPageProps) {
  return (
    <>
      <AccountSettings
        key={user.id}
        account={user.id}
        library={libraries.find((library) => !library.personal)?.id}
      />
      <SharedCorrectionsSettings
        key={`shared-corrections:${user.id}`}
        account={user.id}
      />
    </>
  );
}
