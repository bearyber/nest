import { useEffect, useMemo, useRef, useState } from "react";
import { formatName, isBlank, parseName, type NamePiece } from "../../lib/editorModel";
import { BUILTIN_BLOCKS } from "../../lib/templateDraft";
import type { Field } from "../../lib/types";
import styles from "./editor.module.css";

/** What an answer is called, for its chip. Unknown keys show as they are (Rust says what's wrong). */
export function blankLabel(key: string, fields: Field[]): string {
  if (key === "item") return "each choice";
  if (key === "yymmdd" || key === "date") return "Today";
  const f = fields.find((x) => x.key === key);
  if (f) return f.type === "client" ? "Client" : f.label;
  return BUILTIN_BLOCKS.find((b) => b.key === key)?.label ?? key;
}

const known = (key: string, fields: Field[]) =>
  key === "item" || fields.some((f) => f.key === key) || BUILTIN_BLOCKS.some((b) => b.key === key) || key === "yymmdd";

/** A name with its answers as chips (read-only). Clicking a chip opens its settings. */
export function NameView({
  name,
  fields,
  onChip,
}: {
  name: string;
  fields: Field[];
  onChip?: (key: string, index: number) => void;
}) {
  const pieces = parseName(name);
  return (
    <span className={`${styles.name} ${styles.mono}`}>
      {pieces.map((p, i) =>
        isBlank(p) ? (
          <button
            key={i}
            type="button"
            className={`${styles.chip} ${known(p.key, fields) ? "" : styles.chipUnknown}`}
            onClick={(e) => {
              e.stopPropagation();
              onChip?.(p.key, i);
            }}
          >
            {blankLabel(p.key, fields)}
          </button>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
    </span>
  );
}

/** Text and chips, always alternating and starting and ending with text, so there's always a
 *  place to type before and after each chip. */
function normalize(pieces: NamePiece[]): NamePiece[] {
  const out: NamePiece[] = [];
  for (const p of pieces) {
    const last = out[out.length - 1];
    if (!isBlank(p)) {
      if (last && !isBlank(last)) out[out.length - 1] = { text: last.text + p.text };
      else out.push(p);
    } else {
      if (!last || isBlank(last)) out.push({ text: "" });
      out.push(p);
    }
  }
  if (out.length === 0 || isBlank(out[out.length - 1])) out.push({ text: "" });
  return out;
}

/** Answers a name can use: your questions that make sense in a name, and Nest's own. */
function choices(fields: Field[]): { key: string; label: string; hint: string }[] {
  const yours = fields
    .filter((f) => f.type !== "longtext" && f.type !== "bool")
    .map((f) => ({
      key: f.key,
      label: f.type === "client" ? "Client" : f.label,
      hint: f.type === "date" ? "date" : f.type === "multi" ? "several" : "",
    }));
  const nest = [...BUILTIN_BLOCKS, { key: "yymmdd", label: "Today" }].map((b) => ({ ...b, hint: "Nest" }));
  return [...yours, ...nest];
}

/** The quick ones under a name being edited: the template's own text, date and client
 *  questions, at most three (the rest are under More…). */
export function quickChoices(fields: Field[]): { key: string; label: string }[] {
  return fields
    .filter((f) => f.type === "text" || f.type === "date" || f.type === "client")
    .slice(0, 3)
    .map((f) => ({ key: f.key, label: f.type === "client" ? "Client" : f.label }));
}

/** A place in the name: a text piece, and a range in it (start–end is replaced). */
interface Spot {
  piece: number;
  start: number;
  end: number;
}

interface Picker extends Spot {
  filter: string;
}

interface Props {
  value: string;
  fields: Field[];
  /** Start with the whole name selected, ready to type over (a folder just added). */
  selectAll?: boolean;
  /** Done (Enter, or clicked away). */
  onCommit: (value: string) => void;
  onCancel: () => void;
  /** Enter: done, and a new folder below. */
  onEnter?: (value: string) => void;
  /** Tab / Shift+Tab: done, and move the folder in or out. */
  onTab?: (value: string, out: boolean) => void;
  /** A new question named `label`: returns its key. */
  onNewBlank: (label: string) => string;
  /** A chip was clicked: its key, and its place among the name's pieces. */
  onChip?: (key: string, index: number) => void;
  /** What the field is (read out by screen readers). */
  label: string;
}

/** Edit a name: type text; click one of New Project's answers under it (or type `{`) to put it
 *  in; select some text and click "Use New Project's answer" to swap it for one. Backspace next
 *  to a chip removes it. */
export default function NameField({ value, fields, selectAll, onCommit, onCancel, onEnter, onTab, onNewBlank, onChip, label }: Props) {
  const [pieces, setPieces] = useState<NamePiece[]>(() => normalize(parseName(value)));
  const [picker, setPicker] = useState<Picker | null>(null);
  const [active, setActive] = useState(0);
  const [selection, setSelection] = useState<Spot | null>(null);
  const inputs = useRef<(HTMLInputElement | null)[]>([]);
  const box = useRef<HTMLDivElement>(null);
  /** Where a quick answer goes: the cursor, or the name selected on open (typed over). */
  const spot = useRef<Spot>(
    (() => {
      const last = pieces.length - 1;
      const len = (pieces[last] as { text: string }).text.length;
      return selectAll && pieces.length === 1 ? { piece: 0, start: 0, end: len } : { piece: last, start: len, end: len };
    })(),
  );
  /** The selection made on open isn't one the user made: it doesn't offer "Use New Project's answer". */
  const opening = useRef(!!selectAll);
  const done = useRef(false);

  const current = () => formatName(pieces.filter((p) => isBlank(p) || p.text !== ""));

  // A new folder starts fully selected (type over it); other names with the cursor at the end.
  useEffect(() => {
    const at = spot.current;
    const el = inputs.current[at.piece];
    el?.focus();
    el?.setSelectionRange(at.start, at.end);
  }, []); // only on open

  const focusAt = (piece: number, at: number) => {
    spot.current = { piece, start: at, end: at };
    setTimeout(() => {
      const el = inputs.current[piece];
      el?.focus();
      el?.setSelectionRange(at, at);
    }, 0);
  };

  const finish = (how: "commit" | "enter" | "tab" | "untab" | "cancel") => {
    if (done.current) return;
    done.current = true;
    const v = current();
    if (how === "cancel") onCancel();
    else if (how === "enter" && onEnter) onEnter(v);
    else if ((how === "tab" || how === "untab") && onTab) onTab(v, how === "untab");
    else onCommit(v);
  };

  const options = useMemo(() => {
    const all = choices(fields);
    const f = picker?.filter.trim().toLowerCase() ?? "";
    return f ? all.filter((c) => c.label.toLowerCase().includes(f)) : all;
  }, [fields, picker]);
  const typed = picker?.filter.trim() ?? "";
  const canAdd = typed !== "" && !choices(fields).some((c) => c.label.toLowerCase() === typed.toLowerCase());
  const count = options.length + (canAdd ? 1 : 0);
  const quick = quickChoices(fields);

  /** Put answer `key` in at `at` (replacing what's selected there). */
  const put = (at: Spot, key: string) => {
    const f = fields.find((x) => x.key === key);
    const transform = f?.type === "date" ? "yymmdd" : undefined;
    const text = (pieces[at.piece] as { text: string } | undefined)?.text ?? "";
    const next = [
      ...pieces.slice(0, at.piece),
      { text: text.slice(0, at.start) },
      transform ? { key, transform } : { key },
      { text: text.slice(at.end) },
      ...pieces.slice(at.piece + 1),
    ];
    setPieces(normalize(next));
    setSelection(null);
    opening.current = false;
    focusAt(at.piece + 2, 0);
  };

  const choose = (key: string | null) => {
    if (!picker) return;
    put(picker, key ?? onNewBlank(typed));
    setPicker(null);
  };

  const openPicker = (at: Spot, filter = "") => {
    setPicker({ ...at, filter });
    setActive(0);
  };

  const removeChip = (index: number) => {
    const next = [...pieces];
    next.splice(index, 1);
    const caret = (pieces[index - 1] as { text: string }).text.length;
    setPieces(normalize(next));
    focusAt(index - 1, caret);
  };

  /** A chip's place among the name's pieces as saved (no empty text between chips). */
  const savedIndex = (i: number) => pieces.slice(0, i).filter((p) => isBlank(p) || p.text !== "").length;

  return (
    <div
      ref={box}
      className={styles.field}
      onBlur={(e) => {
        // Clicked outside the field (not on its picker or buttons): done.
        if (!box.current?.contains(e.relatedTarget as Node | null)) finish("commit");
      }}
    >
      <div className={styles.fieldBox} role="group" aria-label={`Editing: ${label}`}>
        {pieces.map((p, i) =>
          isBlank(p) ? (
            <span key={i} className={`${styles.chip} nest-pop`}>
              <button type="button" className={styles.chipX} style={{ opacity: 1 }} onClick={() => onChip?.(p.key, savedIndex(i))}>
                {blankLabel(p.key, fields)}
              </button>
              <button type="button" className={styles.chipX} aria-label={`Remove ${blankLabel(p.key, fields)}`} onClick={() => removeChip(i)}>
                ×
              </button>
            </span>
          ) : (
            <input
              key={i}
              ref={(el) => {
                inputs.current[i] = el;
              }}
              className={styles.piece}
              style={{ width: `${Math.max(1, p.text.length) + 0.5}ch` }}
              value={p.text}
              aria-label={i === 0 ? label : `${label}, more`}
              spellCheck={false}
              onChange={(e) => {
                const raw = e.target.value.replace(/[/\\}]/g, "");
                const brace = raw.indexOf("{");
                const text = brace >= 0 ? raw.slice(0, brace) + raw.slice(brace + 1) : raw;
                const caret = brace >= 0 ? brace : Math.min(e.target.selectionStart ?? text.length, text.length);
                setPieces((ps) => ps.map((x, j) => (j === i ? { text } : x)));
                opening.current = false;
                setSelection(null);
                spot.current = { piece: i, start: caret, end: caret };
                if (brace >= 0) openPicker({ piece: i, start: brace, end: brace });
              }}
              onSelect={(e) => {
                const el = e.currentTarget;
                const start = el.selectionStart ?? 0;
                const end = el.selectionEnd ?? start;
                spot.current = { piece: i, start, end };
                const whole = start === 0 && end === el.value.length;
                if (opening.current && whole) return;
                opening.current = false;
                setSelection(start !== end ? { piece: i, start, end } : null);
              }}
              onKeyDown={(e) => {
                if (picker) {
                  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                    e.preventDefault();
                    setActive((a) => (a + (e.key === "ArrowDown" ? 1 : count - 1)) % Math.max(1, count));
                    return;
                  }
                  if (e.key === "Enter") {
                    e.preventDefault();
                    choose(active < options.length ? options[active].key : canAdd ? null : (options[0]?.key ?? null));
                    return;
                  }
                  if (e.key === "Escape") {
                    e.preventDefault();
                    setPicker(null);
                    return;
                  }
                }
                const el = e.currentTarget;
                const atStart = el.selectionStart === 0 && el.selectionEnd === 0;
                const atEnd = el.selectionStart === el.value.length;
                if (e.key === "Backspace" && atStart && i > 0) {
                  e.preventDefault();
                  removeChip(i - 1);
                } else if (e.key === "Delete" && atEnd && i < pieces.length - 1) {
                  e.preventDefault();
                  removeChip(i + 1);
                } else if (e.key === "ArrowLeft" && atStart && i > 0) {
                  e.preventDefault();
                  const prev = inputs.current[i - 2];
                  focusAt(i - 2, prev?.value.length ?? 0);
                } else if (e.key === "ArrowRight" && atEnd && i < pieces.length - 1) {
                  e.preventDefault();
                  focusAt(i + 2, 0);
                } else if (e.key === "Enter") {
                  e.preventDefault();
                  finish(onEnter ? "enter" : "commit");
                } else if (e.key === "Escape") {
                  e.preventDefault();
                  finish("cancel");
                } else if (e.key === "Tab" && onTab) {
                  e.preventDefault();
                  finish(e.shiftKey ? "untab" : "tab");
                }
              }}
            />
          ),
        )}
      </div>
      {/* Buttons here never take the focus from the name (so it isn't committed). */}
      <div className={`${styles.fillLine} nest-unfold`} onMouseDown={(e) => e.preventDefault()}>
        {selection ? (
          <button
            type="button"
            className={styles.fillLink}
            onClick={() => {
              const text = (pieces[selection.piece] as { text: string }).text.slice(selection.start, selection.end);
              openPicker(selection, text);
            }}
          >
            Use New Project's answer
          </button>
        ) : (
          <>
            <span>Fill in from New Project:</span>
            {quick.map((q, i) => (
              <span key={q.key} className={styles.fillItem}>
                {i > 0 && <span aria-hidden="true">·</span>}
                <button type="button" className={styles.fillLink} onClick={() => put(spot.current, q.key)}>
                  {q.label}
                </button>
              </span>
            ))}
            <span className={styles.fillItem}>
              {quick.length > 0 && <span aria-hidden="true">·</span>}
              <button type="button" className={styles.fillLink} onClick={() => openPicker(spot.current)}>
                More…
              </button>
            </span>
          </>
        )}
      </div>
      {picker && (
        <div className={styles.picker} role="listbox" aria-label="New Project's answers">
          <input
            className={styles.input}
            autoFocus
            value={picker.filter}
            placeholder="Find an answer, or name a new question"
            aria-label="Find an answer, or name a new question"
            onChange={(e) => {
              setPicker({ ...picker, filter: e.target.value });
              setActive(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                e.preventDefault();
                setActive((a) => (a + (e.key === "ArrowDown" ? 1 : count - 1)) % Math.max(1, count));
              } else if (e.key === "Enter") {
                e.preventDefault();
                choose(active < options.length ? options[active].key : canAdd ? null : (options[0]?.key ?? null));
              } else if (e.key === "Escape") {
                e.preventDefault();
                setPicker(null);
                focusAt(picker.piece, picker.start);
              }
            }}
          />
          {options.map((o, i) => (
            <button
              key={o.key}
              type="button"
              role="option"
              aria-selected={i === active}
              className={`${styles.pickItem} ${i === active ? styles.pickActive : ""}`}
              onMouseEnter={() => setActive(i)}
              onClick={() => choose(o.key)}
            >
              {o.label}
              {o.hint && <small>{o.hint}</small>}
            </button>
          ))}
          {canAdd && (
            <button
              type="button"
              role="option"
              aria-selected={active === options.length}
              className={`${styles.pickItem} ${active === options.length ? styles.pickActive : ""}`}
              onMouseEnter={() => setActive(options.length)}
              onClick={() => choose(null)}
            >
              Add “{typed}” as a question
            </button>
          )}
        </div>
      )}
    </div>
  );
}
