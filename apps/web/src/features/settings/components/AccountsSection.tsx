import type { Schema } from "../../../shared/schema";

import AccountSettings from "../../accounts/components/AccountSettings";
import SharedCorrectionsSettings from "../../artists/components/SharedCorrectionsSettings";

export default function AccountsSection({
  user,
  libraries,
}: Schema<"SettingsPageProps">) {
  const shared = libraries.find((library) => !library.personal);
  return (
    <>
      <AccountSettings
        key={user.id}
        account={user.id}
        library={shared?.id}
        libraryName={shared?.name}
      />
      <SharedCorrectionsSettings
        key={`shared-corrections:${user.id}`}
        account={user.id}
      />
    </>
  );
}
