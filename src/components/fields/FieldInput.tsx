import { todayIso } from "../../lib/tree";
import type { ClientValue, Field, FieldValue } from "../../lib/types";
import Icon from "../Icon";
import { Button } from "../ui";
import ClientField from "./ClientField";
import styles from "./fields.module.css";

interface Props {
  field: Field;
  value: FieldValue | undefined;
  onChange: (value: FieldValue) => void;
  onBlur: () => void;
  error?: string;
  clients: ClientValue[];
  /** Label above the control (narrow places like the template editor's preview). */
  stacked?: boolean;
}

/** One template field, rendered by type (build spec §4): label on the left, control on the right. */
export default function FieldInput({ field, value, onChange, onBlur, error, clients, stacked }: Props) {
  const id = `field-${field.key}`;
  const text = typeof value === "string" ? value : "";
  // The client is always the billing party, whatever an older or personal template calls it.
  const label = field.type === "client" ? "Client" : field.label;
  const required = field.required && (
    <span className={styles.required} aria-hidden="true">
      {" "}
      *
    </span>
  );

  let control: React.ReactNode;
  let labelFor: string | undefined = id;
  switch (field.type) {
    case "text":
      control = (
        <input
          id={id}
          className={styles.input}
          value={text}
          maxLength={field.max}
          spellCheck={false}
          autoComplete="off"
          aria-required={field.required}
          onChange={(e) => onChange(e.target.value)}
          onBlur={onBlur}
        />
      );
      break;
    case "longtext":
      control = (
        <textarea
          id={id}
          className={styles.textarea}
          rows={2}
          value={text}
          maxLength={field.max}
          onChange={(e) => onChange(e.target.value)}
          onBlur={onBlur}
        />
      );
      break;
    case "date":
      control = (
        <div className={styles.inline}>
          <input
            id={id}
            type="date"
            className={`${styles.input} ${styles.date}`}
            value={text}
            aria-required={field.required}
            onChange={(e) => onChange(e.target.value)}
            onBlur={onBlur}
          />
          <Button onClick={() => onChange(todayIso())}>Today</Button>
        </div>
      );
      break;
    case "select":
      control = (
        <select
          id={id}
          className={styles.select}
          value={text}
          onChange={(e) => onChange(e.target.value)}
          onBlur={onBlur}
        >
          <option value="">Choose…</option>
          {field.options?.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
      );
      break;
    case "bool":
      labelFor = undefined;
      control = (
        <label className={styles.check}>
          <input
            id={id}
            type="checkbox"
            checked={value === true}
            onChange={(e) => onChange(e.target.checked)}
            onBlur={onBlur}
          />
          {label}
        </label>
      );
      break;
    case "multi": {
      labelFor = undefined;
      const selected = Array.isArray(value) ? value : [];
      const toggle = (option: string) => {
        const next = selected.includes(option)
          ? selected.filter((s) => s !== option)
          : [...selected, option];
        onChange((field.options ?? []).filter((o) => next.includes(o))); // keep template order
      };
      control = (
        <div className={styles.chips} role="group" aria-labelledby={`${id}-label`}>
          {field.options?.map((o) => {
            const on = selected.includes(o);
            return (
              <button
                key={o}
                type="button"
                className={styles.chip}
                aria-pressed={on}
                onClick={() => toggle(o)}
                onBlur={onBlur}
              >
                {on && <Icon name="check" />}
                {o}
              </button>
            );
          })}
        </div>
      );
      break;
    }
    case "client":
      control = (
        <ClientField
          id={id}
          value={isClient(value) ? value : { name: "", code: "" }}
          clients={clients}
          onChange={onChange}
          onBlur={onBlur}
        />
      );
      break;
  }

  return (
    <div className={stacked ? `${styles.row} ${styles.stacked}` : styles.row}>
      {field.type === "bool" ? (
        !stacked && <span />
      ) : labelFor ? (
        <label htmlFor={labelFor} className={styles.label}>
          {label}
          {required}
        </label>
      ) : (
        <span id={`${id}-label`} className={styles.label}>
          {label}
          {required}
        </span>
      )}
      <div className={styles.control}>
        {control}
        {error && (
          <div className={styles.error} role="alert">
            {error}
          </div>
        )}
      </div>
    </div>
  );
}

function isClient(v: FieldValue | undefined): v is ClientValue {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}
