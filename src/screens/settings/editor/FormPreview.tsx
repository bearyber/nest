import { useState } from "react";
import FieldInput from "../../../components/fields/FieldInput";
import { defaultsFor } from "../../../lib/tree";
import type { Field, FieldValue, Values } from "../../../lib/types";
import styles from "./editor.module.css";

/** The New Project form as it will look, made of the real field controls. Try-out only:
 *  nothing typed here is saved. */
export default function FormPreview({ fields }: { fields: Field[] }) {
  const [typed, setTyped] = useState<Values>({});
  const defaults = defaultsFor(fields);
  const value = (f: Field): FieldValue | undefined => typed[f.key] ?? defaults[f.key];

  return (
    <div className={styles.form} aria-label="New Project form preview">
      {fields.map((f) => (
        <FieldInput
          key={`${f.key}-${f.type}`}
          field={f}
          value={value(f) ?? (f.type === "client" ? { name: "", code: "" } : undefined)}
          clients={[]}
          onChange={(v) => setTyped((t) => ({ ...t, [f.key]: v }))}
          onBlur={() => {}}
          stacked
        />
      ))}
    </div>
  );
}
