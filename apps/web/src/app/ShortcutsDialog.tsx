import { isApplePlatform } from "./searchShortcut";
import Dialog from "../shared/ui/Dialog";
import Switch from "../shared/ui/Switch";

type Row = { what: string; keys: string };

const apple = isApplePlatform();

const GROUPS: { title: string; rows: Row[] }[] = [
  {
    title: "Playback",
    rows: [
      { what: "Play or pause", keys: "Space" },
      { what: "Next track", keys: "Shift →" },
      { what: "Previous track", keys: "Shift ←" },
      { what: "Seek, on the seek slider", keys: "← →" },
      { what: "Volume, on the volume slider", keys: "↑ ↓" },
      { what: "Mute", keys: "M" },
      { what: "Like this track", keys: "L" },
    ],
  },
  {
    title: "Go to",
    rows: [
      { what: "Search", keys: apple ? "⌘K" : "Ctrl K" },
      { what: "Home", keys: "G then H" },
      { what: "Library", keys: "G then L" },
      { what: "Plugins", keys: "G then P" },
      { what: "Queue", keys: "Q" },
      { what: "Now playing", keys: "N" },
      { what: "This list", keys: "?" },
    ],
  },
];

export default function ShortcutsDialog({
  singleKeys,
  onSingleKeys,
  onClose,
}: {
  singleKeys: boolean;
  onSingleKeys: (enabled: boolean) => void;
  onClose: () => void;
}) {
  return (
    <Dialog title="Keyboard shortcuts" wide onClose={onClose}>
      <Switch
        label="Single-key shortcuts"
        hint="Space, M, L, Q, N and G then a letter. Shortcuts pause while you type in a field."
        checked={singleKeys}
        onChange={onSingleKeys}
      />
      <div className="shortcut-groups">
        {GROUPS.map((group) => (
          <section key={group.title} className="shortcut-group">
            <h3>{group.title}</h3>
            <dl>
              {group.rows.map((row) => (
                <div key={row.what} className="shortcut-row">
                  <dt>{row.what}</dt>
                  <dd>
                    <kbd>{row.keys}</kbd>
                  </dd>
                </div>
              ))}
            </dl>
          </section>
        ))}
      </div>
      <p className="shortcut-note">
        Arrow keys only act on what has focus: a list, a slider, the queue.
        Dialogs trap focus, close with Esc and hand focus back to the control
        that opened them.
      </p>
      <div className="dialog-actions">
        <button type="button" className="button" onClick={onClose}>
          Close
        </button>
      </div>
    </Dialog>
  );
}
