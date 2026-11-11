export type ImportProblemAction = "edit" | "plugins" | "copy";

export type ImportProblem = {
  title: string;
  text: string;
  actions: ImportProblemAction[];
};

export function importProblem(
  status: number | undefined,
  service: string,
  message: string,
  following: boolean,
): ImportProblem {
  if (status === undefined)
    return {
      title: "Hito didn't get an answer",
      text: "It can't tell yet whether the playlist was saved. Checking again does not import it twice.",
      actions: [],
    };
  if (status === 401 || status === 403)
    return {
      title: `${service} refused`,
      text: "Its connection may need your review. Nothing was saved.",
      actions: ["plugins"],
    };
  if (status === 404)
    return {
      title: `${service} didn't return this playlist`,
      text: "It may be private, a personal mix, or deleted. Use the link of a public playlist. Nothing was saved.",
      actions: ["edit"],
    };
  if (status === 429)
    return {
      title: following
        ? "Hito can't follow more playlists right now"
        : "Hito can't import more playlists right now",
      text: following
        ? "There is a limit for each account and for the server. You can still import this one as a copy."
        : "There is a limit for each account and for the server. Try again later.",
      actions: following ? ["copy"] : [],
    };
  if (status === 502 || status === 503 || status === 504)
    return {
      title: `The ${service} plugin is busy`,
      text: "Nothing was saved. Try again in a moment.",
      actions: [],
    };
  return {
    title: "Hito couldn't import this playlist",
    text: message,
    actions: ["edit"],
  };
}
