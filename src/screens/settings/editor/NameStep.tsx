import { useState } from "react";
import Icon from "../../../components/Icon";
import { Button } from "../../../components/ui";
import {
  BUILTIN_BLOCKS,
  fromBlocks,
  styleChoices,
  toBlocks,
  type NameBlock,
} from "../../../lib/templateDraft";
import type { Template } from "../../../lib/types";
import styles from "./editor.module.css";

interface Props {
  draft: Template;
  /** The example folder name from Rust's preview ("" while it's on its way). */
  folderName: string;
  onChange: (t: Template) => void;
}

/** Step 2: how project folders are named, built by clicking answers. The title in Nest's list
 *  and writing the name as text are under More options. */
export default function NameStep({ draft, folderName, onChange }: Props) {
  // A custom pattern the blocks can't show opens More options, so nothing is hidden.
  const custom = toBlocks(draft.folderName, "_") === null || (!!draft.title && toBlocks(draft.title, " ") === null);
  const [more, setMore] = useState(custom);
  return (
    <>
      <div className={styles.section}>
        <span className={styles.label}>How should the folder be named?</span>
        <div className={styles.bigExample} aria-live="polite">
          <Icon name="folder" className={styles.folderIcon} />
          <b className={styles.mono}>{folderName || "…"}</b>
        </div>
      </div>

      <BlockEditor
        label="Folder name"
        hint="Your answers, joined with _. Pick how each part is written. The job code is saved inside the project, so it doesn't need to be in the name."
        sep="_"
        pattern={draft.folderName}
        draft={draft}
        allowText={more}
        onChange={(folderName) => onChange({ ...draft, folderName })}
      />

      <button type="button" className={styles.disclosure} aria-expanded={more} onClick={() => setMore(!more)}>
        {more ? "▾" : "▸"} More options
      </button>
      {more && (
        <BlockEditor
          label="Title in Nest's project list"
          hint="Usually leave this on Automatic."
          sep=" "
          pattern={draft.title}
          draft={draft}
          automatic
          allowText
          onChange={(title) => onChange({ ...draft, title: title === "" ? undefined : title })}
        />
      )}
    </>
  );
}

interface BlockProps {
  label: string;
  hint: string;
  sep: string;
  pattern: string | undefined;
  draft: Template;
  /** Empty = automatic (the title). */
  automatic?: boolean;
  /** Offer "write it as text" (More options). */
  allowText?: boolean;
  onChange: (pattern: string) => void;
}

/** A name built from blocks (answers and built-ins), each with a style. Patterns the blocks
 *  can't show (literal text…) open as text, so nothing is lost. */
function BlockEditor({ label, hint, sep, pattern, draft, automatic, allowText, onChange }: BlockProps) {
  const blocks = toBlocks(pattern, sep);
  // The title can be left automatic; then there are no parts to choose.
  const isAuto = !!automatic && !pattern;
  const [asText, setAsText] = useState(false);
  const advanced = asText || blocks === null;
  const fieldOf = (key: string) => draft.fields.find((f) => f.key === key);
  const nameOf = (key: string) => {
    const f = fieldOf(key);
    if (f) return f.type === "client" ? "Billed to" : f.label;
    return BUILTIN_BLOCKS.find((b) => b.key === key)?.label ?? key;
  };
  const set = (next: NameBlock[]) => onChange(fromBlocks(next, sep));
  const used = new Set(blocks?.map((b) => b.key) ?? []);
  const addable = [
    ...draft.fields
      .filter((f) => f.type !== "longtext" && f.type !== "bool" && f.type !== "multi")
      .map((f) => ({ key: f.key, label: f.type === "client" ? "Billed to (client name)" : f.label })),
    ...BUILTIN_BLOCKS,
  ].filter((a) => !used.has(a.key));
  const addPart = (key: string) => {
    const f = fieldOf(key);
    const transform = f?.type === "date" ? "yymmdd" : f && sep === "_" ? "caps" : undefined;
    set([...(blocks ?? []), transform ? { key, transform } : { key }]);
  };

  return (
    <div className={styles.section}>
      <span className={styles.label}>{label}</span>
      <p className={styles.hint}>{hint}</p>
      {automatic && (
        <div className={styles.choiceRow} role="radiogroup" aria-label={label}>
          <label className={styles.radio}>
            <input type="radio" name={`auto-${sep}`} checked={isAuto} onChange={() => onChange("")} />
            Automatic: the text answers, joined with spaces
          </label>
          <label className={styles.radio}>
            <input
              type="radio"
              name={`auto-${sep}`}
              checked={!isAuto}
              onChange={() => {
                const first = draft.fields.find((f) => f.type === "text");
                onChange(first ? `{${first.key}}` : "{client}");
              }}
            />
            Choose the parts
          </label>
        </div>
      )}
      {!isAuto && !advanced && blocks && (
        <>
          <div className={styles.chips}>
            {blocks.map((b, i) => {
              const f = fieldOf(b.key);
              const choices = styleChoices(f ? f.type : "builtin");
              return (
                <span key={`${b.key}-${i}`} className={styles.chipGroup}>
                  {i > 0 && <span className={styles.sep}>{sep === " " ? "␣" : sep}</span>}
                  <span className={styles.chip}>
                    {nameOf(b.key)}
                    {choices.length > 1 && (
                      <select
                        aria-label={`How ${nameOf(b.key)} is written`}
                        value={b.transform ?? ""}
                        onChange={(e) =>
                          set(blocks.map((x, j) => (j === i ? { key: x.key, ...(e.target.value ? { transform: e.target.value } : {}) } : x)))
                        }
                      >
                        {choices.map((c) => (
                          <option key={c.value} value={c.value}>
                            {c.label}
                          </option>
                        ))}
                      </select>
                    )}
                    <button
                      type="button"
                      className={styles.x}
                      aria-label={`Remove ${nameOf(b.key)}`}
                      disabled={!!automatic && blocks.length === 1}
                      title={automatic && blocks.length === 1 ? "To use no parts, choose Automatic above" : undefined}
                      onClick={() => set(blocks.filter((_, j) => j !== i))}
                    >
                      ×
                    </button>
                  </span>
                </span>
              );
            })}
            {blocks.length === 0 && <span className={styles.hint}>Click an answer below to start.</span>}
          </div>
          {addable.length > 0 && (
            <div className={styles.chips} role="group" aria-label={`Add to ${label}`}>
              <span className={styles.hintInline}>Click to add:</span>
              {addable.map((a) => (
                <button key={a.key} type="button" className={styles.addChip} onClick={() => addPart(a.key)}>
                  + {a.label}
                </button>
              ))}
            </div>
          )}
        </>
      )}
      {!isAuto && advanced && (
        <input
          className={`${styles.input} ${styles.mono}`}
          value={pattern ?? ""}
          spellCheck={false}
          aria-label={`${label} (as text)`}
          placeholder={automatic ? "Automatic" : undefined}
          onChange={(e) => onChange(e.target.value)}
        />
      )}
      {!isAuto && (allowText || advanced) && (
        <div>
          {advanced ? (
            blocks !== null && (
              <Button className={styles.linkButton} onClick={() => setAsText(false)}>
                Use blocks
              </Button>
            )
          ) : (
            <Button className={styles.linkButton} onClick={() => setAsText(true)}>
              Write it as text instead
            </Button>
          )}
          {advanced && blocks === null && (
            <p className={styles.hint}>
              This name uses a custom pattern, so it's shown as text. Use {"{question}"} for an answer, e.g.{" "}
              <span className={styles.mono}>{"{start|yymmdd}_{name|caps}"}</span>.
            </p>
          )}
        </div>
      )}
    </div>
  );
}
