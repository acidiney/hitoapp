import type { Schema } from "../../../shared/schema";

export default function PluginPersistencePolicy({
  policy,
}: {
  policy: Schema<"PluginInstallation">["manifest"]["content_policy"];
}) {
  if (policy?.source_persistence === "playback_only")
    return (
      <>
        It only plays audio. Hito won't save new copies, and copies you already
        have stay.
      </>
    );
  if (policy?.source_persistence === "explicit_download")
    return (
      <>
        It allows saving audio when you ask. Hito still checks before saving
        anything, and this isn't a licence.
      </>
    );
  return (
    <>
      It doesn't say whether its audio may be saved. Hito still asks before
      saving anything.
    </>
  );
}
