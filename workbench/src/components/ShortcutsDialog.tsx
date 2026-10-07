import { Fragment, useEffect, useRef } from "react";
import { SHORTCUTS } from "../model/shortcuts.ts";

const MOD = /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘" : "Ctrl";

export function ShortcutsDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    if (open && !dialog.current?.open) dialog.current?.showModal();
    if (!open) dialog.current?.close();
  }, [open]);
  return (
    <dialog ref={dialog} className="dialog shortcuts" aria-labelledby="shortcuts-title" onClose={onClose}>
      <header className="bar">
        <h2 id="shortcuts-title">Keyboard shortcuts</h2>
        <form method="dialog">
          <button type="submit">Close</button>
        </form>
      </header>
      <dl>
        {SHORTCUTS.map(({ keys, action }) => (
          <Fragment key={action}>
            <dt className="row">
              {keys.map((key) => (
                <kbd key={key}>{key.replace("Mod", MOD)}</kbd>
              ))}
            </dt>
            <dd>{action}</dd>
          </Fragment>
        ))}
      </dl>
    </dialog>
  );
}
