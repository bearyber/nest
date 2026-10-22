import { useEffect, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { Button } from "../../components/ui";
import { devicesView, setCodeSettings } from "../../lib/commands";
import { useTauriListener } from "../../lib/useTauriListener";
import type { ComputerLetter, DevicesView } from "../../lib/types";
import type { TabProps } from "../Settings";
import styles from "./settings.module.css";

export type CodePart = { kind: "client" | "marker" | "seq" | "text"; text: string };

/** Split a job code pattern into labelled parts with sample values (Rust validates on save). */
export function codeParts(pattern: string, client: string, marker: string, seq: number): CodePart[] {
  const parts: CodePart[] = [];
  for (const m of pattern.matchAll(/\{clientCode\}|\{marker\}|\{seq(?::(\d+))?\}|[^{]+|\{/g)) {
    if (m[0] === "{clientCode}") parts.push({ kind: "client", text: client });
    else if (m[0] === "{marker}") parts.push({ kind: "marker", text: marker.toUpperCase() });
    else if (m[0].startsWith("{seq")) parts.push({ kind: "seq", text: String(seq).padStart(Number(m[1] ?? 1), "0") });
    else parts.push({ kind: "text", text: m[0] });
  }
  return parts;
}

/** Preview a job code pattern with sample values. */
export function previewCode(pattern: string, client: string, marker: string, seq: number): string {
  return codeParts(pattern, client, marker, seq)
    .map((p) => p.text)
    .join("");
}

/** The ready-made styles. Every one keeps the computer's letter, so two computers never clash. */
export const CODE_STYLES = [
  { pattern: "{clientCode}-{marker}{seq:02}", note: "Recommended" },
  { pattern: "{clientCode}-{marker}{seq:03}", note: "For 100+ jobs per client" },
  { pattern: "{clientCode}{marker}{seq:02}", note: "No dash" },
];

/** A warning with its fix as buttons. The box and its `role="status"` sentence stay in place and
 *  only the words change (so typing a letter doesn't make a new element each time); the buttons
 *  are outside the status text so they aren't read out as part of it. */
function Warning({ text, children }: { text: string | null; children?: ReactNode }) {
  return (
    <div className={styles.warning} hidden={!text}>
      <span role="status">{text}</span>
      {text && <div className={styles.actions}>{children}</div>}
    </div>
  );
}

/** "your PC", "your PC and your Mac". */
export function yourComputers(names: string[]): string {
  return names.map((n) => `your ${n}`).join(" and ");
}

/** Two letters with no dash (or other text) between the client code and the letter can be read
 *  two ways (ACM + EW, or ACME + W), so Settings warns and offers a dash. Never refuses. */
export function separatorWarning(pattern: string, marker: string): string | null {
  if (marker.length !== 2 || !/\{clientCode\}\{marker\}|\{marker\}\{clientCode\}/.test(pattern)) return null;
  const example = previewCode(pattern, "ACME", marker, 4);
  return `With two letters and no dash, a code like ${example} can be read two ways (the client's code and the letters run together). Put a dash between them?`;
}

/** The same style with a dash between the client code and the letter. */
export function withSeparator(pattern: string): string {
  return pattern.replace(/\{clientCode\}\{marker\}/g, "{clientCode}-{marker}").replace(/\{marker\}\{clientCode\}/g, "{marker}-{clientCode}");
}

/** The other computers using this letter, or none. */
export function computersWithLetter(letters: ComputerLetter[], marker: string): ComputerLetter[] {
  return marker ? letters.filter((l) => l.letter.toUpperCase() === marker.toUpperCase()) : [];
}

const CAPTION: Record<CodePart["kind"], string> = {
  client: "Client code",
  marker: "This computer",
  seq: "Job number",
  text: "",
};

const INSERTS = [
  { label: "Client code", text: "{clientCode}" },
  { label: "Letter", text: "{marker}" },
  { label: "Number (2 digits)", text: "{seq:02}" },
  { label: "Number (3 digits)", text: "{seq:03}" },
  { label: "-", text: "-" },
];

/** Job codes, in plain words: a live example, a few styles, the letter for this computer and the
 *  code for personal jobs. The raw pattern lives under Advanced. */
export default function CodesTab({ view, run }: TabProps) {
  const s = view.settings;
  const [pattern, setPattern] = useState(s.codePattern);
  const [marker, setMarker] = useState(s.localMarker);
  const [own, setOwn] = useState(s.ownCode);
  const isStyle = CODE_STYLES.some((c) => c.pattern === pattern);
  const [advanced, setAdvanced] = useState(!isStyle);
  const changed = pattern !== s.codePattern || marker !== s.localMarker || own !== s.ownCode;
  const letter = marker || "?";
  const parts = codeParts(pattern, "ACME", letter, 4);
  const noClient = s.noClientSpaces[0] ?? "Personal";
  const markerProblem =
    marker.startsWith("J")
      ? "J is kept for jobs from your job tracker, so a letter can't start with J. Pick another."
      : marker.length > 2
        ? `Older versions allowed "${marker}". Use 1 or 2 letters to save changes.`
        : marker.length === 0
          ? "Every computer needs one letter."
          : null;
  const patternProblem = pattern.trim() ? null : "The format can't be empty.";

  // Your other computers' letters (Devices must be on to know them).
  const [dev, setDev] = useState<DevicesView | null>(null);
  const loadDevices = () => devicesView().then(setDev).catch(() => {});
  useEffect(() => {
    void loadDevices();
  }, [s.localMarker]);
  useTauriListener(
    (cb) => listen("devices-changed", cb),
    () => void loadDevices(),
  );
  /** "Keep W": the warning for that letter stays quiet until the letter changes. */
  const [kept, setKept] = useState<string | null>(null);
  const takenBy = markerProblem ? [] : computersWithLetter(dev?.letters ?? [], marker);
  const suggested = dev?.suggestedLetter ?? "";
  const devicesOn = dev?.enabled === true;
  const noLetter = devicesOn && !patternProblem && !pattern.includes("{marker}");
  const separator = patternProblem ? null : separatorWarning(pattern, marker);
  /** One button, one change: only what the warning is about is saved (other edits stay as typed). */
  const saveOne = (p: string, m: string) =>
    run(setCodeSettings(p, m, s.ownCode), `Saved. Your next code will look like ${previewCode(p, "ACME", m, 4)}.`);

  return (
    <>
      <p className={styles.hint}>
        Every project gets a short code you can put on invoices and use to find the job later. Nest picks the next free
        number by itself. The code is saved inside the project, so your folder names stay the way you like them.
      </p>

      <section className={`${styles.card} ${styles.codeHero}`} aria-label="Example job code">
        <span className={styles.sectionTitle}>Example: your 4th job for a client called Acme</span>
        <div className={styles.codeParts} aria-live="polite">
          {parts.map((p, i) => (
            <span key={i} className={p.kind === "text" ? styles.codeSep : styles.codePart}>
              <b>{p.text}</b>
              {p.kind !== "text" && <small>{CAPTION[p.kind]}</small>}
            </span>
          ))}
        </div>
      </section>

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>How codes look</h3>
        <div className={styles.choices} role="radiogroup" aria-label="Code style">
          {CODE_STYLES.map((c) => (
            <label key={c.pattern} className={styles.choice}>
              <input
                type="radio"
                name="codeStyle"
                checked={pattern === c.pattern}
                onChange={() => {
                  setPattern(c.pattern);
                  setAdvanced(false);
                }}
              />
              <b className={styles.mono}>{previewCode(c.pattern, "ACME", letter, 4)}</b>
              <small>{c.note}</small>
            </label>
          ))}
        </div>
      </section>

      <Warning
        text={
          noLetter
            ? `This style leaves out the computer's letter, so two of your computers could make the same code (like ${previewCode(pattern, "ACME", letter, 4)} on both). Use ${previewCode(CODE_STYLES[0].pattern, "ACME", letter, 4)} instead?`
            : null
        }
      >
        <Button onClick={() => void saveOne(CODE_STYLES[0].pattern, s.localMarker).then((r) => r !== undefined && setPattern(CODE_STYLES[0].pattern))}>
          Use {previewCode(CODE_STYLES[0].pattern, "ACME", letter, 4)}
        </Button>
      </Warning>
      <Warning text={separator}>
        <Button
          onClick={() => {
            const fixed = withSeparator(pattern);
            void saveOne(fixed, s.localMarker).then((r) => r !== undefined && setPattern(fixed));
          }}
        >
          Add a dash
        </Button>
      </Warning>

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>The parts you can change</h3>
        <div className={styles.card}>
          <div className={styles.field}>
            <span className={styles.name}>Client code</span>
            <p className={styles.hint}>
              Comes from the client (who pays you). You give each client a code the first time (for example Acme →{" "}
              <b className={styles.mono}>ACME</b>). Nothing to set here.
            </p>
          </div>
          <div className={styles.field}>
            <label className={styles.name} htmlFor="marker">
              Letter for this computer
            </label>
            <input
              id="marker"
              className={`${styles.input} ${styles.codeInput}`}
              value={marker}
              maxLength={marker.length > 2 ? undefined : 2}
              aria-describedby="marker-hint"
              onChange={(e) => setMarker(e.target.value.toUpperCase().replace(/[^A-Z]/g, ""))}
            />
            <p className={styles.hint} id="marker-hint">
              Each of your computers needs its own letter, so two computers never make the same code. Use 1 or 2 letters,
              and a different one on each (for example W on your PC, M on your Mac).
            </p>
            {markerProblem && <p className={styles.problem}>{markerProblem}</p>}
            {!markerProblem && marker !== s.localMarker && (
              <p className={styles.hint}>
                Your numbers carry on from the old letter ({s.localMarker}05 is followed by {marker}06). Nothing already
                made is renamed.
              </p>
            )}
            <Warning
              text={
                takenBy.length > 0 && kept !== marker
                  ? `${marker} ${takenBy.some((l) => l.guessed) ? "seems to be used" : "is used"} by ${yourComputers(takenBy.map((l) => l.name))}.${suggested ? ` Use ${suggested} instead?` : ""}`
                  : null
              }
            >
              {suggested && (
                <Button onClick={() => void saveOne(s.codePattern, suggested).then((r) => r !== undefined && setMarker(suggested))}>
                  Use {suggested}
                </Button>
              )}
              <Button onClick={() => setKept(marker)}>Keep {marker}</Button>
            </Warning>
            {!markerProblem && s.localMarker === "L" && marker === "L" && (
              <p className={styles.note}>
                L was the old default. If another of your computers also uses L, change one of them.
              </p>
            )}
          </div>
          <div className={styles.field}>
            <label className={styles.name} htmlFor="own">
              Code for personal jobs
            </label>
            <input
              id="own"
              className={`${styles.input} ${styles.codeInput}`}
              value={own}
              maxLength={5}
              aria-describedby="own-hint"
              onChange={(e) => setOwn(e.target.value.toUpperCase().replace(/[^A-Z0-9]/g, ""))}
            />
            <p className={styles.hint} id="own-hint">
              Jobs in a space with no client (like {noClient}) have nobody to bill, so they use this instead. Your
              initials or studio name work well. Example:{" "}
              <b className={styles.mono}>{previewCode(pattern, own || "OWN", letter, 2)}</b>
            </p>
          </div>
        </div>
      </section>

      <details className={styles.advanced} open={advanced} onToggle={(e) => setAdvanced(e.currentTarget.open)}>
        <summary>Advanced: write your own format</summary>
        <div className={`${styles.card} ${styles.field}`}>
          <p className={styles.hint}>Only if none of the styles above fit. Click a part to add it.</p>
          <div className={styles.inserts}>
            {INSERTS.map((p) => (
              <Button key={p.label} onClick={() => setPattern((v) => v + p.text)}>
                {p.label}
              </Button>
            ))}
            <Button onClick={() => setPattern("")}>Clear</Button>
          </div>
          <input
            className={`${styles.input} ${styles.mono}`}
            value={pattern}
            spellCheck={false}
            aria-label="Code format"
            onChange={(e) => setPattern(e.target.value)}
          />
          <p className={styles.hint}>
            Looks like: <b className={styles.mono}>{previewCode(pattern, "ACME", letter, 4)}</b>
          </p>
          {patternProblem && <p className={styles.problem}>{patternProblem}</p>}
        </div>
      </details>

      <div className={styles.actions}>
        <Button
          variant="primary"
          disabled={!changed || !!markerProblem || !!patternProblem}
          onClick={() => run(setCodeSettings(pattern, marker, own), `Saved. Your next code will look like ${previewCode(pattern, "ACME", marker, 4)}.`)}
        >
          Save
        </Button>
        <Button
          disabled={!changed}
          onClick={() => {
            setPattern(s.codePattern);
            setMarker(s.localMarker);
            setOwn(s.ownCode);
            setAdvanced(!CODE_STYLES.some((c) => c.pattern === s.codePattern));
          }}
        >
          Undo changes
        </Button>
      </div>
    </>
  );
}
