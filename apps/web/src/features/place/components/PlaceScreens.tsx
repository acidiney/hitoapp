import { useEffect, useState } from "react";
import type { ServerPlace } from "../api/desktopPlace";
import ConnectServer from "./ConnectServer";
import FirstOpen from "./FirstOpen";
import SavedServer from "./SavedServer";

type Screen =
  | { kind: "loading" }
  | { kind: "choose" }
  | { kind: "server" }
  | { kind: "saved"; origin: string };

export default function PlaceScreens({
  place,
  useThisComputer,
}: {
  place: ServerPlace;
  useThisComputer: () => Promise<void>;
}) {
  const [screen, setScreen] = useState<Screen>({ kind: "loading" });
  useEffect(() => {
    let current = true;
    place
      .saved()
      .then((origin) => {
        if (current)
          setScreen(origin ? { kind: "saved", origin } : { kind: "choose" });
      })
      .catch(() => current && setScreen({ kind: "choose" }));
    return () => {
      current = false;
    };
  }, [place]);
  switch (screen.kind) {
    case "loading":
      return null;
    case "saved":
      return (
        <SavedServer
          place={place}
          origin={screen.origin}
          onChangeServer={() => setScreen({ kind: "choose" })}
        />
      );
    case "server":
      return (
        <ConnectServer
          place={place}
          onBack={() => setScreen({ kind: "choose" })}
        />
      );
    case "choose":
      return (
        <FirstOpen
          useThisComputer={useThisComputer}
          chooseServer={() => setScreen({ kind: "server" })}
        />
      );
  }
}
