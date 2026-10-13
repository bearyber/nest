import { useState } from "react";
import { Button } from "../../components/ui";
import { retype, styleChoices } from "../../lib/templateDraft";
import type { Field, FieldKind } from "../../lib/types";
import { blankLabel } from "./NameField";
import styles from "./editor.module.css";

export const KINDS: { kind: FieldKind; label: string }[] = [
  { kind: "text", label: "Text" },
  { kind: "date", label: "Date" },
  { kind: "select", label: "Pick one" },
  { kind: "multi", label: "Pick several" },
  { kind: "bool", label: "Yes / no" },
  { kind: "longtext", label: "Longer text (notes)" },
];

/** "Must be filled in" fits answers that can be left empty. */
const canRequire = (k: FieldKind) => k === "text" || k === "longtext" || k === "date" || k === "select";

interface Props {
  blankKey: string;
  fields: Field[];
  /** Opened from a chip: how the blank is written there (its transform), and a way to change it. */
  written?: { transform?: string; onChange: (transform: string | undefined) => void };
  onChange: (field: Field) => void;
  onRemove: (key: string) => void;
  onClose: () => void;
}

/** One blank's settings, under the tree: its name, what kind of answer it is, whether it must be
 *  filled in, its choices, and (opened from a chip) how it's written in that name. */
export default function BlankSettings({ blankKey, fields, written, onChange, onRemove, onClose }: Props) {
  const f = fields.find((x) => x.key === blankKey);
  const builtin = !f;
  const isClient = f?.type === "client";
  const kind: FieldKind | "builtin" = f ? f.type : "builtin";
  const styleKind = kind === "client" || kind === "select" || kind === "multi" ? "text" : kind;
  const writtenChoices = builtin ? [] : styleChoices(styleKind as FieldKind);

  return (
    <section className={styles.settings} aria-label={`Blank: ${blankLabel(blankKey, fields)}`}>
      <div className={styles.settingsRow}>
        {f && !isClient ? (
          <input
            className={styles.input}
            value={f.label}
            maxLength={40}
            aria-label="Blank name"
            onChange={(e) => onChange({ ...f, label: e.target.value })}
          />
        ) : (
          <strong>{blankLabel(blankKey, fields)}</strong>
        )}
        {isClient && <span>Makes the job code. For personal jobs, tick “No client” in New Project.</span>}
        {builtin && <span>Filled in by Nest.</span>}
        {f && !isClient && (
          <label>
            Answer
            <select
              className={styles.select}
              value={f.type}
              onChange={(e) => onChange(retype(f, e.target.value as FieldKind))}
            >
              {KINDS.map((k) => (
                <option key={k.kind} value={k.kind}>
                  {k.label}
                </option>
              ))}
            </select>
          </label>
        )}
        {f && !isClient && canRequire(f.type) && (
          <label title="New Project won't create until this is filled in">
            <input
              type="checkbox"
              checked={!!f.required}
              onChange={(e) => onChange({ ...f, required: e.target.checked || undefined })}
            />
            Must be filled in
          </label>
        )}
        {written && writtenChoices.length > 0 && (
          <label>
            Written here as
            <select
              className={styles.select}
              value={written.transform ?? ""}
              onChange={(e) => written.onChange(e.target.value || undefined)}
            >
              {writtenChoices.map((c) => (
                <option key={c.value} value={c.value}>
                  {c.label}
                </option>
              ))}
            </select>
          </label>
        )}
        <span className={styles.spacer} />
        {f && !isClient && <Button onClick={() => onRemove(f.key)}>Remove blank</Button>}
        <Button onClick={onClose}>Done</Button>
      </div>
      {f && !isClient && <Extras field={f} onChange={onChange} />}
    </section>
  );
}

/** Choices and starting values, per kind of answer. */
function Extras({ field: f, onChange }: { field: Field; onChange: (f: Field) => void }) {
  const [adding, setAdding] = useState("");
  const options = f.options ?? [];
  const multiDefault = Array.isArray(f.default) ? (f.default as string[]) : [];

  const addOption = () => {
    const v = adding.trim();
    if (!v || options.includes(v)) return;
    onChange({ ...f, options: [...options, v] });
    setAdding("");
  };
  const removeOption = (o: string) => {
    const next = options.filter((x) => x !== o);
    let def = f.default;
    if (f.type === "multi") def = multiDefault.filter((x) => x !== o);
    if (f.type === "select" && def === o) def = undefined;
    onChange({ ...f, options: next, ...(def === undefined ? { default: undefined } : { default: def }) });
  };

  if (f.type === "multi" || f.type === "select")
    return (
      <div className={styles.settingsRow}>
        Choices:
        {options.map((o) => {
          const on = f.type === "multi" ? multiDefault.includes(o) : f.default === o;
          return (
            <span key={o} className={styles.option}>
              <button
                type="button"
                className={on ? styles.optionOn : styles.optionOff}
                aria-pressed={on}
                title={on ? "Starts picked. Click to change." : "Click to have it picked to start with"}
                onClick={() => {
                  if (f.type === "multi")
                    onChange({ ...f, default: on ? multiDefault.filter((x) => x !== o) : [...multiDefault, o] });
                  else onChange({ ...f, default: on ? undefined : o });
                }}
              >
                {o}
              </button>
              <button
                type="button"
                className={styles.x}
                aria-label={`Remove choice ${o}`}
                disabled={options.length <= 1}
                onClick={() => removeOption(o)}
              >
                ×
              </button>
            </span>
          );
        })}
        <input
          className={styles.input}
          value={adding}
          placeholder="+ Add a choice"
          aria-label="Add a choice"
          maxLength={30}
          onChange={(e) => setAdding(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addOption();
            }
          }}
          onBlur={addOption}
        />
        <span>Highlighted choices start picked.</span>
      </div>
    );
  if (f.type === "bool")
    return (
      <div className={styles.settingsRow}>
        <label>
          <input type="checkbox" checked={f.default === true} onChange={(e) => onChange({ ...f, default: e.target.checked })} />
          Ticked to start with
        </label>
      </div>
    );
  if (f.type === "date")
    return (
      <div className={styles.settingsRow}>
        <label>
          Starts as
          <select
            className={styles.select}
            value={f.default === "today" ? "today" : ""}
            onChange={(e) => onChange({ ...f, default: e.target.value === "today" ? "today" : undefined })}
          >
            <option value="today">Today</option>
            <option value="">Empty</option>
          </select>
        </label>
      </div>
    );
  return null;
}
