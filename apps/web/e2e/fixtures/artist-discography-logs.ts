const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;
type Family =
  | "artist_review"
  | "artist_discography"
  | "artist_editions"
  | "artist_edition_track_list";

export function artistDiscographyLogs(editions: boolean) {
  const started = new Map<string, { family: Family; action: string }>();
  const finished = new Set<string>();
  const acceptedReview = {
    load: 0,
    search: 0,
    preview: 0,
    confirm: 0,
    profile: 0,
  };
  const acceptedCatalog = { browse: 0, more: 0 };
  const acceptedEditions = { browse: 0, more: 0 };
  let acceptedTracks = 0;

  function observe(text: string) {
    try {
      const values: unknown = JSON.parse(text);
      if (!Array.isArray(values) || values[0] !== "[hito]") return true;
      const event = values[1] as Record<string, unknown> | null;
      if (
        !event ||
        typeof event !== "object" ||
        typeof event.event !== "string"
      )
        return true;
      const match =
        /^(artist_review|artist_discography|artist_editions|artist_edition_track_list)\.(started|finished)$/.exec(
          event.event,
        );
      if (!match) return true;
      const family = match[1] as Family;
      const beginning = match[2] === "started";
      const review = family === "artist_review";
      const tracks = family === "artist_edition_track_list";
      if (!editions && (tracks || family === "artist_editions")) return false;
      const keys = beginning
        ? tracks
          ? ["event", "request_id"]
          : ["event", "action", "request_id"]
        : review
          ? ["event", "action", "request_id", "reason", "duration_ms"]
          : ["event", "request_id", "reason", "duration_ms", "count"];
      if (
        Object.keys(event).length !== keys.length ||
        Object.keys(event).some((key) => !keys.includes(key)) ||
        typeof event.request_id !== "string" ||
        !uuid.test(event.request_id)
      )
        return false;
      if (beginning) {
        const actions = review
          ? Object.keys(acceptedReview)
          : tracks
            ? ["read"]
            : ["browse", "more"];
        const action = tracks ? "read" : String(event.action);
        if (
          !actions.includes(action) ||
          started.has(event.request_id) ||
          started.size >= 32
        )
          return false;
        started.set(event.request_id, { family, action });
        return true;
      }
      const first = started.get(event.request_id);
      const maximum = tracks ? 4096 : 20;
      if (
        !first ||
        first.family !== family ||
        finished.has(event.request_id) ||
        event.reason !== "accepted" ||
        typeof event.duration_ms !== "number" ||
        !Number.isFinite(event.duration_ms) ||
        event.duration_ms < 0 ||
        (review && event.action !== first.action) ||
        (!review &&
          (typeof event.count !== "number" ||
            !Number.isInteger(event.count) ||
            event.count < 0 ||
            event.count > maximum)) ||
        (tracks && event.count !== 4) ||
        (family === "artist_editions" && event.count !== 1)
      )
        return false;
      if (review) acceptedReview[first.action as keyof typeof acceptedReview]++;
      else if (family === "artist_discography")
        acceptedCatalog[first.action as keyof typeof acceptedCatalog]++;
      else if (family === "artist_editions")
        acceptedEditions[first.action as keyof typeof acceptedEditions]++;
      else acceptedTracks++;
      finished.add(event.request_id);
      return true;
    } catch {
      return false;
    }
  }

  return {
    observe,
    acceptedReview,
    acceptedCatalog,
    complete: () =>
      started.size === (editions ? 13 : 10) &&
      finished.size === started.size &&
      acceptedReview.load === 2 &&
      acceptedReview.search === 1 &&
      acceptedReview.preview === 1 &&
      acceptedReview.confirm === 1 &&
      acceptedReview.profile === 2 &&
      acceptedCatalog.browse === 2 &&
      acceptedCatalog.more === 1 &&
      acceptedEditions.browse === (editions ? 1 : 0) &&
      acceptedEditions.more === (editions ? 1 : 0) &&
      acceptedTracks === (editions ? 1 : 0),
  };
}
