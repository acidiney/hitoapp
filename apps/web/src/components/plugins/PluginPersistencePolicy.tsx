import type { PluginInstallation } from "../../types";

export default function PluginPersistencePolicy({
  policy,
}: {
  policy: PluginInstallation["manifest"]["content_policy"];
}) {
  if (policy?.source_persistence === "playback_only")
    return (
      <>
        Playback only. No new offline downloads. Completed copies are retained.
      </>
    );
  if (policy?.source_persistence === "explicit_download")
    return (
      <>
        Explicit downloads declared, subject to Hito's checks. This does not
        grant a licence.
      </>
    );
  return (
    <>
      No persistence declaration. Download permission is unknown; Hito's checks
      still apply.
    </>
  );
}
