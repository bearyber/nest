import { useEffect, useRef, useState } from "react";
import type { Template } from "../lib/types";
import Icon from "./Icon";
import styles from "./TemplatePicker.module.css";

/** `{start|yymmdd}_{name|caps}` → `YYMMDD_NAME`: a readable hint of the folder name. */
export function patternHint(folderName: string): string {
  return folderName.replace(/\{(\w+)(?:\|(\w+))?\}/g, (_, name: string, t?: string) =>
    t === "yymmdd" ? "YYMMDD" : name === "jobCode" ? "CODE" : name.toUpperCase(),
  );
}

interface Props {
  templates: Template[];
  value: string;
  onChange: (id: string) => void;
}

/** Template button in the sheet header, with a menu showing what each template makes. */
export default function TemplatePicker({ templates, value, onChange }: Props) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const current = templates.find((t) => t.id === value);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  const show = () => {
    setActive(Math.max(0, templates.findIndex((t) => t.id === value)));
    setOpen(true);
  };
  const pick = (id: string) => {
    onChange(id);
    setOpen(false);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!open) {
      if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        show();
      }
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, templates.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      pick(templates[active].id);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation(); // close the menu, not the sheet
      setOpen(false);
    }
  };

  return (
    <div className={styles.picker} ref={root}>
      <button
        type="button"
        className={styles.trigger}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`Template: ${current?.name ?? "none"}`}
        onClick={() => (open ? setOpen(false) : show())}
        onKeyDown={onKeyDown}
      >
        <span className={styles.name}>{current?.name ?? "Choose template"}</span>
        {current && <span className={styles.hint}>· {patternHint(current.folderName)}</span>}
        <Icon name="down" className={styles.chevron} />
      </button>
      {open && (
        <ul className={styles.menu} role="listbox" aria-label="Templates">
          {templates.map((t, i) => (
            <li
              key={t.id}
              role="option"
              aria-selected={t.id === value}
              className={i === active ? styles.active : undefined}
              onMouseEnter={() => setActive(i)}
              onMouseDown={(e) => {
                e.preventDefault();
                pick(t.id);
              }}
            >
              <span className={styles.optionName}>
                {t.name}
                {t.id === value && <Icon name="check" className={styles.tick} />}
              </span>
              <span className={styles.optionSub}>
                {patternHint(t.folderName)}
                {t.description ? ` · ${t.description}` : ""}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
