import { Button } from "./ui";
import type { WhatsNew as WhatsNewNote } from "../lib/types";
import styles from "./WhatsNew.module.css";

/** The small "What's new" sheet shown once after an update. Lines are plain text. */
export default function WhatsNew({ note, onOk }: { note: WhatsNewNote; onOk: () => void }) {
  return (
    <div className={styles.backdrop}>
      <div
        className={styles.sheet}
        role="dialog"
        aria-modal="true"
        aria-labelledby="whats-new-title"
        onKeyDown={(e) => {
          if (e.key === "Escape") onOk();
        }}
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
          <Button autoFocus variant="primary" onClick={onOk}>
            OK
          </Button>
        </div>
      </div>
    </div>
  );
}
