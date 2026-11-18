import { createRoot } from "react-dom/client";
import PlaceScreens from "../features/place/components/PlaceScreens";
import { desktopPlace, serverPlace } from "../features/place/api/desktopPlace";

const root = document.getElementById("first-open");
if (root) {
  createRoot(root).render(
    <PlaceScreens
      place={serverPlace}
      useThisComputer={desktopPlace.useThisComputer}
    />,
  );
}
