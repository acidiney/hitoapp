import type { BrowserContext } from "@playwright/test";
import type { LibraryNativeWitness } from "./audiobook-library-delivery";

type Entry = {
  text: string;
  location: string;
  type: string;
  args: number;
  witness: LibraryNativeWitness | null;
  locationOwned: boolean;
  blockedMessage: boolean;
  script: boolean;
};
export function libraryConsoleBoundary(
  context: BrowserContext,
  origin: string,
) {
  let witnessFor: (location: string) => LibraryNativeWitness | null = () =>
    null;
  let invalid = 0;
  const entries: Entry[] = [];
  const snapshots = new Set<string>();
  context.on("console", (message) => {
    try {
      const text = message.text();
      const location = message.location().url;
      if (
        entries.length >= 256 ||
        Buffer.byteLength(text) > 16_384 ||
        Buffer.byteLength(location) > 8192
      ) {
        invalid++;
        return;
      }
      const direct = witnessFor(location);
      const prefix = "Web Inspector blocked ";
      const suffix = " from loading";
      const embedded =
        location === "" &&
        message.type() === "info" &&
        text.startsWith(prefix) &&
        text.endsWith(suffix)
          ? text.slice(prefix.length, -suffix.length)
          : "";
      const blocked = embedded ? witnessFor(embedded) : null;
      entries.push({
        text,
        location,
        type: message.type(),
        args: message.args().length,
        witness: direct ?? blocked,
        locationOwned: direct !== null,
        blockedMessage: blocked !== null,
        script:
          location.startsWith(`${origin}/assets/`) &&
          new URL(location).pathname.endsWith(".js"),
      });
    } catch {
      invalid++;
    }
  });
  return {
    networkProof(classify: typeof witnessFor) {
      witnessFor = classify;
    },
    snapshot(text: string) {
      if (snapshots.size >= 256 && !snapshots.has(text)) invalid++;
      else snapshots.add(text);
    },
    inspect(privateValues: Set<string>, qualifyNative: boolean) {
      const contains = (text: string) =>
        [...privateValues].some((value) => text.includes(value));
      let privateConsole = 0;
      let nativeNetworkDiagnostics = 0;
      const findings: Record<string, unknown>[] = [];
      for (const snapshot of snapshots)
        if (contains(snapshot)) {
          privateConsole++;
          if (findings.length < 10)
            findings.push({
              source: "application_snapshot",
              application_log: true,
            });
        }
      for (const entry of entries) {
        if (!contains(entry.text) && !contains(entry.location)) continue;
        const witness = entry.witness;
        const accepted =
          witness?.acceptedStatus === (witness?.phase === "anchor" ? 409 : 200);
        const controlled =
          witness?.phase === "anchor" || witness?.failed === true;
        const qualified =
          qualifyNative &&
          entry.args === 0 &&
          !entry.script &&
          (entry.type === "error" || entry.type === "info") &&
          (entry.locationOwned || entry.blockedMessage) &&
          witness?.observed === true &&
          accepted &&
          controlled;
        if (qualified) nativeNetworkDiagnostics++;
        else privateConsole++;
        if (findings.length < 10)
          findings.push({
            route: witness ? "/audiobooks" : "other",
            type: entry.type,
            args_count: entry.args,
            location_class: entry.locationOwned
              ? "owned_request"
              : entry.script
                ? "app_script"
                : "other",
            location_empty: entry.location === "",
            exact_native_message_url: entry.blockedMessage,
            exact_owned_request: witness?.observed ?? false,
            expected_status: accepted,
            phase: witness?.phase ?? "other",
            controlled_failure: witness?.failed ?? false,
            application_log: entry.text.startsWith("[hito]") || entry.script,
            classified_native: qualified,
          });
      }
      return { privateConsole, nativeNetworkDiagnostics, invalid, findings };
    },
  };
}
