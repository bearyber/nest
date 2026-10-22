import type { ComputerLetter } from "../lib/types";
import { Button } from "./ui";
import styles from "../screens/Main.module.css";

/** "Your PC and this Mac both use W." (or "seem to", when a computer's letter is only judged
 *  from its newest codes). */
export function letterNoteText(clash: ComputerLetter[], letter: string, mac: boolean): string {
  const names = clash.map((c) => c.name);
  const who = names.length <= 1 ? names[0] : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
  const verb = clash.some((c) => c.guessed) ? "seem to both use" : "both use";
  return `Your ${who} and this ${mac ? "Mac" : "PC"} ${verb} ${letter}. Change this computer's letter?`;
}

/** Shown when another of your computers makes job codes with the same letter as this one.
 *  Nothing changes by itself: "Change letter…" opens Settings → Job codes with a free letter
 *  suggested; "Later" hides this until the situation changes or Nest restarts. */
export default function LetterNote({
  clash,
  letter,
  mac,
  onChange,
  onLater,
}: {
  clash: ComputerLetter[];
  letter: string;
  mac: boolean;
  onChange: () => void;
  onLater: () => void;
}) {
  return (
    <div className={styles.update} role="status">
      <span>{letterNoteText(clash, letter, mac)}</span>
      <span className={styles.noteActions}>
        <Button onClick={onChange}>Change letter…</Button>
        <Button onClick={onLater}>Later</Button>
      </span>
    </div>
  );
}
