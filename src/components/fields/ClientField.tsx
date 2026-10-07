import { useState } from "react";
import type { ClientValue } from "../../lib/types";
import styles from "./fields.module.css";

interface Props {
  id: string;
  value: ClientValue;
  clients: ClientValue[];
  onChange: (value: ClientValue) => void;
  onBlur: () => void;
}

const MAX_SUGGESTIONS = 5;

/** Client name + 2–5 char code, suggesting clients already used in the jobs folder. */
export default function ClientField({ id, value, clients, onChange, onBlur }: Props) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);

  const query = value.name.trim().toLowerCase();
  const suggestions = query
    ? clients
        .filter(
          (c) =>
            (c.name.toLowerCase().includes(query) || c.code.toLowerCase().startsWith(query)) &&
            !(c.name === value.name && c.code === value.code),
        )
        .slice(0, MAX_SUGGESTIONS)
    : [];
  const showList = open && suggestions.length > 0;

  const pick = (c: ClientValue) => {
    onChange({ name: c.name, code: c.code });
    setOpen(false);
  };

  const setName = (name: string) => {
    // Typing a known client's exact name fills its code if none is set yet.
    const known = clients.find((c) => c.name.toLowerCase() === name.trim().toLowerCase());
    onChange({ name, code: value.code || known?.code || "" });
    setOpen(true);
    setActive(0);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!showList) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, suggestions.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === "Enter" && !(e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      pick(suggestions[active]);
    } else if (e.key === "Escape") {
      e.stopPropagation(); // close the list, not the sheet
      setOpen(false);
    }
  };

  return (
    <div className={styles.client}>
      <div className={styles.clientInputs}>
        <input
          id={id}
          className={styles.input}
          placeholder="Name"
          value={value.name}
          spellCheck={false}
          autoComplete="off"
          role="combobox"
          aria-expanded={showList}
          aria-controls={`${id}-list`}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={onKeyDown}
          onFocus={() => setOpen(true)}
          onBlur={() => {
            setOpen(false);
            onBlur();
          }}
        />
        <input
          aria-label="Client code"
          className={`${styles.input} ${styles.code}`}
          placeholder="CODE"
          value={value.code}
          maxLength={5}
          spellCheck={false}
          autoComplete="off"
          onChange={(e) =>
            onChange({ name: value.name, code: e.target.value.toUpperCase().replace(/[^A-Z0-9]/g, "") })
          }
          onBlur={onBlur}
        />
      </div>
      {showList && (
        <ul id={`${id}-list`} role="listbox" className={styles.suggestions}>
          {suggestions.map((c, i) => (
            <li
              key={c.code}
              role="option"
              aria-selected={i === active}
              className={i === active ? styles.activeSuggestion : undefined}
              // mousedown, not click: fires before the input's blur closes the list
              onMouseDown={(e) => {
                e.preventDefault();
                pick(c);
              }}
            >
              {c.name} <span className={styles.suggestionCode}>{c.code}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
