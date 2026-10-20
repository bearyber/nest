import { useEffect, useRef } from "react";
import { isTopDialog } from "../lib/dialogStack";
import { useDialogFocus } from "../lib/useDialogFocus";
import { Button } from "./ui";
import type { WhatsNew as WhatsNewNote } from "../lib/types";
import styles from "./WhatsNew.module.css";

/** The small "What's new" sheet shown once after an update. Lines are plain text. */
export default function WhatsNew({ note, onOk }: { note: WhatsNewNote; onOk: () => void }) {
  const sheetRef = useRef<HTMLDivElement>(null);
  // Focus starts on OK (the default button), stays in the sheet, and goes back when it closes.
  useDialogFocus(sheetRef);
  // Return or Escape closes it, like a native sheet's default button.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!isTopDialog(sheetRef.current)) return;
      if ((e.key === "Enter" || e.key === "Escape") && !e.defaultPrevented) {
        e.preventDefault();
        onOk();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onOk]);
  return (
    <div className={styles.backdrop}>
      <div
        ref={sheetRef}
        tabIndex={-1}
        className={styles.sheet}
        role="dialog"
        aria-modal="true"
        aria-labelledby="whats-new-title"
      >
        <h2 id="whats-new-title" className={styles.title}>
          What&rsquo;s new in Nest {note.version}
        </h2>
        <ul className={styles.list}>
          {note.lines.map((line, i) => (
            <li key={i}>{line}</li>
          ))}
        </ul>
        <div className={styles.foot}>
          <Button variant="primary" data-autofocus onClick={onOk}>
            OK
          </Button>
        </div>
      </div>
    </div>
  );
}
