import { Button } from "../../components/ui";
import { openTemplates } from "../../lib/commands";
import type { TabProps } from "../Settings";
import styles from "./settings.module.css";

/** Templates live in the main window now (PLAN 2026-10-13); this tab points there. */
export default function TemplatesTab({ run }: TabProps) {
  return (
    <>
      <p className={styles.hint}>
        Templates are in the main window now: click Templates in its sidebar. You can make, change, import and export
        them there.
      </p>
      <div className={styles.actions}>
        <Button variant="primary" onClick={() => void run(openTemplates())}>
          Open templates
        </Button>
      </div>
    </>
  );
}
