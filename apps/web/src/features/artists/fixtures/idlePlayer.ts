export const idlePlayer = {
  subscribe: () => () => {},
  snapshot: () => ({ current: null, shuffle: false }),
  queueSnapshot: () => ({
    version: 0,
    total: 0,
    currentIndex: null,
    currentEntry: null,
    pending: 0,
  }),
  prepareSelection: () => async () => true,
  shuffle: () => {},
};

export const idlePlayerModule = async () => {
  const { idlePlayer: player } = await import("./idlePlayer");
  return { usePlayer: () => player };
};
