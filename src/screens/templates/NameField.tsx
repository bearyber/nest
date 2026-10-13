import { useEffect, useMemo, useRef, useState } from "react";
import { formatName, isBlank, parseName, type NamePiece } from "../../lib/editorModel";
import { BUILTIN_BLOCKS } from "../../lib/templateDraft";
import type { Field } from "../../lib/types";
import styles from "./editor.module.css";

/** What a blank is called, for its chip. Unknown keys show as they are (Rust says what's wrong). */
export function blankLabel(key: string, fields: Field[]): string {
  if (key === "item") return "each choice";
  if (key === "yymmdd" || key === "date") return "Today";
  const f = fields.find((x) => x.key === key);
  if (f) return f.type === "client" ? "Client" : f.label;
  return BUILTIN_BLOCKS.find((b) => b.key === key)?.label ?? key;
}

const known = (key: string, fields: Field[]) =>
  key === "item" || fields.some((f) => f.key === key) || BUILTIN_BLOCKS.some((b) => b.key === key) || key === "yymmdd";

/** A name with its blanks as chips (read-only). Clicking a chip opens its settings. */
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

/** Blanks a name can use: your questions that make sense in a name, and Nest's own. */
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

interface Picker {
  /** The text piece the blank goes into, and where in it (start–end: replaced by "Make a blank"). */
  piece: number;
  start: number;
  end: number;
  filter: string;
}

interface Props {
  value: string;
  fields: Field[];
  /** Done (Enter, or clicked away). */
  onCommit: (value: string) => void;
  onCancel: () => void;
  /** Enter: done, and a new folder below. */
  onEnter?: (value: string) => void;
  /** Tab / Shift+Tab: done, and move the folder in or out. */
  onTab?: (value: string, out: boolean) => void;
  /** A new blank named `label`: returns its key. */
  onNewBlank: (label: string) => string;
  onChip?: (key: string) => void;
  /** Characters a name can't have (e.g. "/" in a folder name). */
  label: string;
}

/** Edit a name: type text; type `{` (or click Add blank) to put in a blank; select some text and
 *  click Make a blank to turn it into one. Backspace next to a chip removes it. */
export default function NameField({ value, fields, onCommit, onCancel, onEnter, onTab, onNewBlank, onChip, label }: Props) {
  const [pieces, setPieces] = useState<NamePiece[]>(() => normalize(parseName(value)));
  const [picker, setPicker] = useState<Picker | null>(null);
  const [active, setActive] = useState(0);
  const [selection, setSelection] = useState<{ piece: number; start: number; end: number } | null>(null);
  const inputs = useRef<(HTMLInputElement | null)[]>([]);
  const box = useRef<HTMLDivElement>(null);
  const lastCaret = useRef<{ piece: number; at: number }>({ piece: 0, at: 0 });
  const done = useRef(false);

  const current = () => formatName(pieces.filter((p) => isBlank(p) || p.text !== ""));

  // Start with the cursor at the end, everything selected (like renaming in Finder / Explorer).
  useEffect(() => {
    const last = inputs.current[inputs.current.length - 1];
    if (pieces.length === 1) inputs.current[0]?.select();
    else last?.focus();
    // Only on open.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const focusAt = (piece: number, at: number) =>
    setTimeout(() => {
      const el = inputs.current[piece];
      el?.focus();
      el?.setSelectionRange(at, at);
    }, 0);

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

  const choose = (key: string | null) => {
    if (!picker) return;
    const k = key ?? onNewBlank(typed);
    const f = fields.find((x) => x.key === k);
    const transform = f?.type === "date" ? "yymmdd" : undefined;
    const text = (pieces[picker.piece] as { text: string }).text;
    const next = [
      ...pieces.slice(0, picker.piece),
      { text: text.slice(0, picker.start) },
      transform ? { key: k, transform } : { key: k },
      { text: text.slice(picker.end) },
      ...pieces.slice(picker.piece + 1),
    ];
    setPieces(normalize(next));
    setPicker(null);
    setSelection(null);
    focusAt(picker.piece + 2, 0);
  };

  const openPicker = (piece: number, start: number, end: number, filter = "") => {
    setPicker({ piece, start, end, filter });
    setActive(0);
  };

  const removeChip = (index: number) => {
    const next = [...pieces];
    next.splice(index, 1);
    const caret = (pieces[index - 1] as { text: string }).text.length;
    setPieces(normalize(next));
    focusAt(index - 1, caret);
  };

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
            <span key={i} className={styles.chip}>
              <button type="button" className={styles.chipX} style={{ opacity: 1 }} onClick={() => onChip?.(p.key)}>
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
                setPieces((ps) => ps.map((x, j) => (j === i ? { text } : x)));
                if (brace >= 0) openPicker(i, brace, brace);
              }}
              onSelect={(e) => {
                const el = e.currentTarget;
                const start = el.selectionStart ?? 0;
                const end = el.selectionEnd ?? start;
                lastCaret.current = { piece: i, at: end };
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
      {selection ? (
        <button
          type="button"
          className={`${styles.small}`}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => {
            const text = (pieces[selection.piece] as { text: string }).text.slice(selection.start, selection.end);
            openPicker(selection.piece, selection.start, selection.end, text);
          }}
        >
          Make a blank
        </button>
      ) : (
        <button
          type="button"
          className={styles.small}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => openPicker(lastCaret.current.piece, lastCaret.current.at, lastCaret.current.at)}
        >
          Add blank
        </button>
      )}
      {picker && (
        <div className={styles.picker} role="listbox" aria-label="Blanks">
          <input
            className={styles.input}
            autoFocus
            value={picker.filter}
            placeholder="Find or name a blank"
            aria-label="Find or name a blank"
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
              Add “{typed}” as a blank
            </button>
          )}
        </div>
      )}
    </div>
  );
}
