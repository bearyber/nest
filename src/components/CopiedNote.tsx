import { useState } from "react";
import type { ComputerLetter } from "../lib/types";
import { Button } from "./ui";
import styles from "../screens/Main.module.css";

/** "This Mac was copied from another Mac, so Nest has given it its own place in your list." */
export function copiedNoteText(mac: boolean): string {
  const what = mac ? "Mac" : "PC";
  return `This ${what} was copied from another ${what}, so Nest has given it its own place in your list. Give it a name:`;
}

/** The copy still has the original's job-code letter: say so, and offer a free one. */
export function copiedLetterText(clash: ComputerLetter[], letter: string, mac: boolean): string {
  const names = clash.map((c) => c.name);
  const who = names.length <= 1 ? names[0] : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
  const verb = clash.some((c) => c.guessed) ? "seem to both use" : "both use";
  return `Your ${who} and this ${mac ? "Mac" : "PC"} ${verb} the letter ${letter} in job codes.`;
}

/** Shown once, after Nest found this computer to be a copy and gave it its own id (nothing
 *  else changed by itself). Saving the name closes it. If the letter is the same as the
 *  original's, the same one-button suggestion as in Settings → Job codes is offered here. */
export default function CopiedNote({
  initialName,
  mac,
  clash,
  letter,
  suggested,
  onSave,
  onUseLetter,
}: {
  initialName: string;
  mac: boolean;
  clash: ComputerLetter[];
  letter: string;
  suggested: string;
  onSave: (name: string) => void;
  onUseLetter: (letter: string) => void;
}) {
  const [name, setName] = useState(initialName);
  const trimmed = name.trim();
  const ok = trimmed.length > 0 && trimmed.length <= 40;
  return (
    <div className={styles.update} role="status">
      <div className={styles.noteBody}>
        <span>{copiedNoteText(mac)}</span>
        <span className={styles.noteActions}>
          <input
            aria-label="Name for this computer"
            value={name}
            maxLength={40}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && ok) onSave(trimmed);
            }}
          />
          <Button disabled={!ok} onClick={() => onSave(trimmed)}>
            Save
          </Button>
        </span>
        {clash.length > 0 && suggested && (
          <span className={styles.noteActions}>
            <span>{copiedLetterText(clash, letter, mac)}</span>
            <Button onClick={() => onUseLetter(suggested)}>Use {suggested}</Button>
          </span>
        )}
      </div>
    </div>
  );
}
