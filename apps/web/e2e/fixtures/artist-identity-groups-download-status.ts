type Counts = {
  download: number;
  downloadStatusReads: number;
  downloadStatusAbsentReplies: number;
  invalidDownloadStatus: number;
  bounds: number;
};
type StatusRequest = {
  url: string;
  method: string;
  body: string | null;
  headers: Readonly<Record<string, string>>;
};
type StatusReply = {
  status: number;
  headers: Readonly<Record<string, string>>;
  body(): Promise<Uint8Array>;
};
type Witness = {
  path: string;
  account: string;
  authorized: boolean | null;
  reply: "pending" | "valid" | "invalid";
  started: boolean;
  consumed: boolean;
};
const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;

export function artistGroupDownloadStatus(origin: string, counts: Counts) {
  const witnesses = new Map<number, Witness>();
  let target: { account: string; paths: Set<string> } | null = null;
  let finalized = false;

  function consume(witness: Witness) {
    if (witness.consumed || witness.authorized === null) return;
    if (witness.authorized && witness.reply === "pending") return;
    witness.consumed = true;
    if (!witness.authorized) return;
    if (witness.reply === "valid") counts.downloadStatusAbsentReplies++;
    else counts.invalidDownloadStatus++;
  }
  function authorize(witness: Witness) {
    if (!target || witness.authorized !== null) return;
    witness.authorized =
      witness.account === target.account && target.paths.has(witness.path);
    if (witness.authorized) counts.downloadStatusReads++;
    else counts.download++;
    consume(witness);
  }

  return {
    request(input: StatusRequest): number | null {
      const url = new URL(input.url);
      if (!/\/(?:download|downloads)(?:\/|$)/.test(url.pathname)) return null;
      const account = input.headers["x-hito-account"];
      if (
        finalized ||
        url.origin !== origin ||
        url.search ||
        url.hash ||
        url.username ||
        url.password ||
        input.method !== "GET" ||
        input.body !== null ||
        input.headers["x-hito-request"] !== "1" ||
        !account ||
        !uuid.test(account) ||
        !/^\/api\/v1\/(recordings|playlists)\/[^/]+\/download$/.test(
          url.pathname,
        )
      ) {
        counts.download++;
        return null;
      }
      if (witnesses.size >= 32) {
        counts.bounds++;
        return null;
      }
      const ticket = witnesses.size;
      const witness: Witness = {
        path: url.pathname,
        account,
        authorized: null,
        reply: "pending",
        started: false,
        consumed: false,
      };
      witnesses.set(ticket, witness);
      authorize(witness);
      return ticket;
    },
    async reply(ticket: number, input: StatusReply) {
      const witness = witnesses.get(ticket);
      if (!witness || witness.started || finalized) {
        counts.invalidDownloadStatus++;
        return;
      }
      witness.started = true;
      if (witness.authorized === false) return;
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        const length = input.headers["content-length"];
        const mime = input.headers["content-type"]
          ?.split(";", 1)[0]
          .trim()
          .toLowerCase();
        if (
          input.status !== 404 ||
          mime !== "application/json" ||
          input.headers["cache-control"] !== "private, no-store" ||
          (length !== undefined &&
            (!/^(0|[1-9][0-9]*)$/.test(length) || Number(length) > 4096))
        )
          throw new Error("Invalid owned status envelope");
        const bytes = await Promise.race([
          input.body(),
          new Promise<never>((_resolve, reject) => {
            timer = setTimeout(
              () => reject(new Error("Owned status response timed out")),
              3000,
            );
          }),
        ]);
        if (bytes.length !== 30)
          throw new Error("Invalid owned status body length");
        const value: unknown = JSON.parse(new TextDecoder().decode(bytes));
        witness.reply =
          typeof value === "object" &&
          value !== null &&
          Object.keys(value).length === 1 &&
          "error" in value &&
          value.error === "Resource not found"
            ? "valid"
            : "invalid";
      } catch {
        witness.reply = "invalid";
      } finally {
        clearTimeout(timer);
      }
      consume(witness);
    },
    watch(account: string, recordings: string[], playlist: string) {
      if (
        target ||
        finalized ||
        !uuid.test(account) ||
        recordings.length !== 4 ||
        new Set(recordings).size !== 4 ||
        recordings.some((recording) => !uuid.test(recording)) ||
        !uuid.test(playlist)
      )
        throw new Error("Invalid owned status authorization");
      target = {
        account,
        paths: new Set([
          ...recordings.map(
            (recording) => `/api/v1/recordings/${recording}/download`,
          ),
          `/api/v1/playlists/${playlist}/download`,
        ]),
      };
      for (const witness of witnesses.values()) authorize(witness);
    },
    finalize() {
      if (finalized) return;
      finalized = true;
      for (const witness of witnesses.values()) {
        if (!witness.consumed) {
          counts.invalidDownloadStatus++;
          witness.consumed = true;
        }
      }
    },
  };
}
