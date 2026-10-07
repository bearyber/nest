import { Segmented } from "../../components/ui";
import { setTheme } from "../../lib/commands";
import type { Settings } from "../../lib/types";
import type { TabProps } from "../Settings";
import styles from "./settings.module.css";

const OPTIONS: { value: Settings["theme"]; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

/** Appearance: follow the system, or always light / dark. */
export default function AppearanceTab({ view, run }: TabProps) {
  const current = OPTIONS.find((o) => o.value === view.settings.theme) ?? OPTIONS[0];
  return (
    <section className={styles.section}>
      <div style={{ maxWidth: 360 }}>
        <Segmented
          label="Appearance"
          block
          options={OPTIONS.map((o) => o.label)}
          value={current.label}
          onChange={(label) => {
            const next = OPTIONS.find((o) => o.label === label);
            if (next) void run(setTheme(next.value));
          }}
        />
      </div>
      <p className={styles.hint}>System follows your computer's light or dark setting.</p>
    </section>
  );
}
