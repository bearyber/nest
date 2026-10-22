import { fileManager } from "../lib/platform";
import { Button } from "./ui";
import styles from "./FirstSteps.module.css";

const STEPS = [
  { title: "1. Answer a few questions", hint: "Client, project name, date." },
  { title: "2. Nest makes the folders", hint: "Named the same way, every time." },
  { title: "3. Find it here later", hint: "Search, status, sizes." },
];

interface Props {
  /** Opens New Project. */
  onStart: () => void;
  /** Opens New Project filled in with an example (nothing is made until Create). */
  onExample: () => void;
}

/** The main window before there are any projects at all: three steps and two buttons. */
export default function FirstSteps({ onStart, onExample }: Props) {
  return (
    <div className={`${styles.guide} nest-in`}>
      <h2 className={styles.title}>Make your first project</h2>
      <p className={styles.subtitle}>Three steps, about a minute.</p>
      <ol className={styles.steps}>
        {STEPS.map((s) => (
          <li key={s.title} className={styles.step}>
            <span className={styles.stepTitle}>{s.title}</span>
            <span className={styles.stepHint}>{s.hint}</span>
          </li>
        ))}
      </ol>
      <div className={styles.buttons}>
        <Button onClick={onExample}>Try an example</Button>
        <Button variant="primary" onClick={onStart}>
          Make your first project
        </Button>
      </div>
    </div>
  );
}

interface NoteProps {
  code: string;
  /** Where the project folder is, already shortened for one line. */
  folder: string;
  /** The full path, as the tooltip. */
  fullPath?: string;
  onReveal: () => void;
  onDone: () => void;
}

/** Shown once, after the very first project: it's a normal folder, and where it is. */
export function FirstProjectNote({ code, folder, fullPath, onReveal, onDone }: NoteProps) {
  return (
    <div className={`${styles.note} nest-in`} role="status">
      <div className={styles.noteText}>
        <b>{code} is ready</b>
        <span title={fullPath}>It's a normal folder in {folder}. Put your files in it, Nest keeps track.</span>
      </div>
      <Button onClick={onReveal}>{`Show in ${fileManager()}`}</Button>
      <Button variant="primary" onClick={onDone}>
        Got it
      </Button>
    </div>
  );
}
