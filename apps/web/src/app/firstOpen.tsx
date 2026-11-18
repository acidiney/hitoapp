import { createRoot } from "react-dom/client";
import FirstOpen from "../features/place/components/FirstOpen";
import { desktopPlace } from "../features/place/api/desktopPlace";

const root = document.getElementById("first-open");
if (root) {
  createRoot(root).render(
    <FirstOpen
      useThisComputer={desktopPlace.useThisComputer}
      chooseServer={() => {}}
    />,
  );
}
