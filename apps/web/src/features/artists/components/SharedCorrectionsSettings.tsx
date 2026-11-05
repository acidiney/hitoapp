import Button from "../../../shared/ui/Button";
import HealthDot from "../../../shared/ui/HealthDot";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { useSharedCorrections } from "../hooks/useSharedCorrections";
import { setting } from "../../settings/model/settingsCatalog";

export default function SharedCorrectionsSettings({
  account,
}: {
  account: string;
}) {
  const corrections = useSharedCorrections(account);
  const { page } = corrections;
  const busy = corrections.pending !== null || corrections.retrying;
  return (
    <SettingsCard setting={setting("shared-corrections")} busy={busy}>
      {corrections.failed && (
        <SectionWarning
          tone="error"
          title="Hito could not read the shared corrections."
        />
      )}
      {page && page.items.length === 0 && (
        <p>No corrections are shared with every account.</p>
      )}
      {page && page.items.length > 0 && (
        <ul className="plain-list">
          {page.items.map((item) => (
            <li key={item.id}>
              <SettingRow
                title={`${item.kind === "unlink" ? "Unlink" : "Split"} · ${item.recordings} ${item.recordings === 1 ? "recording" : "recordings"}`}
                hint={
                  item.retired > 0
                    ? `${item.retired} no longer ${item.retired === 1 ? "matches" : "match"} the file tags`
                    : undefined
                }
                status={
                  <HealthDot
                    tone={item.applied ? "success" : "warning"}
                    label={item.applied ? "Applied" : "Withdrawn"}
                  />
                }
                action={
                  <Button
                    disabled={busy}
                    onClick={() => void corrections.toggle(item)}
                  >
                    {item.applied ? "Withdraw" : "Restore"}
                    {" shared correction "}
                    {page.items.indexOf(item) + 1 + page.offset}
                  </Button>
                }
              />
            </li>
          ))}
        </ul>
      )}
      {page && page.total > page.limit && (
        <nav className="form-actions" aria-label="Shared correction pages">
          <Button disabled={page.offset === 0} onClick={corrections.previous}>
            Previous
          </Button>
          <Button
            disabled={page.offset + page.limit >= page.total}
            onClick={corrections.next}
          >
            Next
          </Button>
        </nav>
      )}
      {corrections.error && <p role="alert">{corrections.error}</p>}
      {corrections.retrying && (
        <Button onClick={() => void corrections.retry()}>
          Retry the same shared correction request
        </Button>
      )}
      {corrections.notice && <p role="status">{corrections.notice}</p>}
    </SettingsCard>
  );
}
