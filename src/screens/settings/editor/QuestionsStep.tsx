import { useState } from "react";
import { Menu } from "@tauri-apps/api/menu";
import { Button } from "../../../components/ui";
import { blankField, isUsed, removeQuestion, retype, usage, type Usage } from "../../../lib/templateDraft";
import type { Field, FieldKind, Template } from "../../../lib/types";
import { usePointerDrag } from "./drag";
import styles from "./editor.module.css";

interface Props {
  draft: Template;
  onChange: (t: Template) => void;
}

const TYPES: { kind: FieldKind; label: string; example: string; hint: string }[] = [
  { kind: "text", label: "Name or title", example: "e.g. Artist, Film title", hint: "Can go in the folder name (step 2)." },
  { kind: "date", label: "Date", example: "e.g. Shoot date", hint: "Can go in the folder name, like 260302." },
  { kind: "select", label: "Pick one", example: "e.g. Camera: Alexa, RED, FX6", hint: "One choice is picked. Can go in the folder name." },
  { kind: "multi", label: "Pick several", example: "e.g. Formats: 16x9, 9x16 (one folder each)", hint: "Can make one folder per choice (step 3)." },
  { kind: "bool", label: "Yes / no", example: "e.g. Has VFX? (makes a VFX folder)", hint: "Can decide whether a folder gets made (step 3)." },
  { kind: "longtext", label: "Notes", example: "e.g. Brief, links", hint: "Saved with the project, not used in folder names." },
];

/** "Required" makes sense for answers that can be left empty. */
const canRequire = (k: FieldKind) => k === "text" || k === "longtext" || k === "date" || k === "select";

function usedText(u: Usage): string[] {
  const out: string[] = [];
  if (u.inFolderName) out.push("In the folder name");
  if (u.inTitle) out.push("In the title");
  if (u.folders) out.push(`Decides ${u.folders} folder${u.folders === 1 ? "" : "s"}`);
  if (u.names) out.push(`Names ${u.names} folder${u.names === 1 ? "" : "s"} or file${u.names === 1 ? "" : "s"}`);
  return out;
}

/** Step 1: the questions New Project asks. One line each; click a line for its details.
 *  Drag to reorder. Client is always there (New Project can tick "No client"). */
export default function QuestionsStep({ draft, onChange }: Props) {
  const [confirm, setConfirm] = useState<string | null>(null);
  const [open, setOpen] = useState<Set<string>>(new Set());
  const fields = draft.fields;
  const setFields = (next: Field[]) => onChange({ ...draft, fields: next });
  const update = (key: string, f: Field) => setFields(fields.map((x) => (x.key === key ? f : x)));
  const toggle = (key: string) =>
    setOpen((s) => {
      const next = new Set(s);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const add = (kind: FieldKind) => {
    const f = blankField(
      kind,
      fields.map((x) => x.key),
    );
    setFields([...fields, f]);
    setOpen((s) => new Set(s).add(f.key));
  };
  const addMenu = async () => {
    try {
      const menu = await Menu.new({
        items: TYPES.map((t) => ({ id: t.kind, text: `${t.label}  (${t.example})`, action: () => add(t.kind) })),
      });
      await menu.popup();
    } catch (e) {
      console.warn("Add a question: menu didn't open", e);
      add("text"); // no native menu (e.g. tests): a plain question
    }
  };

  const drag = usePointerDrag({
    attr: "data-question",
    canDrop: (id, target) => id !== target,
    onDrop: (id, t) => {
      const moving = fields.find((f) => f.key === id);
      if (!moving) return;
      const rest = fields.filter((f) => f.key !== id);
      const at = rest.findIndex((f) => f.key === t.id) + (t.where === "after" ? 1 : 0);
      setFields([...rest.slice(0, at), moving, ...rest.slice(at)]);
    },
  });

  return (
    <div className={styles.section}>
      <span className={styles.label}>What should New Project ask?</span>
      <p className={styles.hint}>Drag ⋮⋮ to change the order. Click ▸ for more about a question.</p>
      <div className={styles.cards}>
        {fields.map((f) => {
          const locked = f.type === "client";
          const u = usage(draft, f.key);
          const type = TYPES.find((t) => t.kind === f.type);
          const drop = drag.target?.id === f.key ? drag.target.where : null;
          const isOpen = open.has(f.key);
          return (
            <div
              key={f.key}
              data-question={f.key}
              className={`${styles.card} ${drag.dragging === f.key ? styles.dragging : ""} ${
                drop === "before" ? styles.dropBefore : drop === "after" ? styles.dropAfter : ""
              }`}
              {...drag.handlers(f.key)}
            >
              <div className={styles.cardTop}>
                <span className={styles.handle} aria-hidden="true">
                  ⋮⋮
                </span>
                <input
                  className={`${styles.input} ${styles.qLabel}`}
                  value={locked ? "Client" : f.label}
                  disabled={locked}
                  maxLength={40}
                  aria-label="Question"
                  onChange={(e) => update(f.key, { ...f, label: e.target.value })}
                />
                {locked ? (
                  <span className={styles.lock}>Makes the job code. For personal jobs, tick “No client” in New Project.</span>
                ) : (
                  <>
                    <select
                      className={styles.typeSelect}
                      aria-label="Answer type"
                      value={f.type}
                      onChange={(e) => {
                        const kind = e.target.value as FieldKind;
                        update(f.key, retype(f, kind));
                        // Choices need setting up: show them.
                        if (kind === "select" || kind === "multi") setOpen((s) => new Set(s).add(f.key));
                      }}
                    >
                      {TYPES.map((t) => (
                        <option key={t.kind} value={t.kind}>
                          {t.label}
                        </option>
                      ))}
                    </select>
                    <label className={styles.reqSwitch} title="New Project won't create until this is filled in">
                      <input
                        type="checkbox"
                        role="switch"
                        checked={!!f.required}
                        disabled={!canRequire(f.type)}
                        onChange={(e) => update(f.key, { ...f, required: e.target.checked || undefined })}
                      />
                      Required
                    </label>
                    <button
                      type="button"
                      className={styles.more}
                      aria-expanded={isOpen}
                      aria-label={`More about ${f.label}`}
                      onClick={() => toggle(f.key)}
                    >
                      {isOpen ? "▾" : "▸"}
                    </button>
                    <button
                      type="button"
                      className={styles.x}
                      aria-label={`Remove ${f.label}`}
                      onClick={() => (isUsed(u) ? setConfirm(f.key) : onChange(removeQuestion(draft, f.key)))}
                    >
                      ×
                    </button>
                  </>
                )}
              </div>
              {!locked && isOpen && (
                <div className={styles.cardMore}>
                  {type && (
                    <p className={styles.hint}>
                      {type.example}. {type.hint}
                    </p>
                  )}
                  <Extras field={f} onChange={(next) => update(f.key, next)} />
                  {usedText(u).length > 0 && (
                    <div className={styles.useds}>
                      {usedText(u).map((t) => (
                        <span key={t} className={styles.used}>
                          {t}
                        </span>
                      ))}
                    </div>
                  )}
                </div>
              )}
              {confirm === f.key && (
                <div className={styles.confirm} role="alert">
                  <span>
                    “{f.label}” is used ({usedText(u).join(", ").toLowerCase()}). Removing it changes those too. Remove
                    anyway?
                  </span>
                  <Button onClick={() => setConfirm(null)}>Keep it</Button>
                  <Button
                    onClick={() => {
                      setConfirm(null);
                      onChange(removeQuestion(draft, f.key));
                    }}
                  >
                    Remove
                  </Button>
                </div>
              )}
            </div>
          );
        })}
      </div>
      <div>
        <Button onClick={() => void addMenu()}>+ Add a question ▾</Button>
      </div>
    </div>
  );
}

/** Choices, starting values and "must be filled in", per answer type. */
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

  return (
    <div className={styles.extras}>
      {(f.type === "multi" || f.type === "select") && (
        <div className={styles.options}>
          <span className={styles.olabel}>Choices:</span>
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
            className={`${styles.input} ${styles.addOption}`}
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
          <span className={styles.hintInline}>Highlighted choices start picked.</span>
        </div>
      )}
      {f.type === "bool" && (
        <label className={styles.check}>
          <input type="checkbox" checked={f.default === true} onChange={(e) => onChange({ ...f, default: e.target.checked })} />
          Ticked to start with
        </label>
      )}
      {f.type === "date" && (
        <label className={styles.check}>
          Starts as
          <select
            className={styles.smallSelect}
            value={f.default === "today" ? "today" : ""}
            onChange={(e) => onChange({ ...f, default: e.target.value === "today" ? "today" : undefined })}
          >
            <option value="today">Today</option>
            <option value="">Empty</option>
          </select>
        </label>
      )}
    </div>
  );
}
