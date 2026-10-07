import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { Button } from "../../components/ui";
import { checkForUpdates, openLogsFolder, revealTemplatesFolder } from "../../lib/commands";
import type { TabProps } from "../Settings";
import styles from "./settings.module.css";

/** About: version, check for updates, logs and the templates folder (support). */
export default function AboutTab({ run }: TabProps) {
  const [version, setVersion] = useState("");
  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  return (
    <>
      <section className={styles.section}>
        <div className={styles.preview}>
          <span className={styles.name}>Nest by INTRVL</span>
          <span className={styles.sub}>Version {version || "…"}</span>
        </div>
        <div className={styles.actions}>
          <Button
            onClick={async () => {
              const u = await run(checkForUpdates());
              if (u === null) void run(Promise.resolve(null), "Nest is up to date");
              else if (u) void run(Promise.resolve(null), `Nest ${u.version} is ready. Use "Restart to update" in the main window.`);
            }}
          >
            Check for updates
          </Button>
          <Button onClick={() => run(openLogsFolder())}>Open logs folder</Button>
          <Button onClick={() => run(revealTemplatesFolder())}>Open templates folder</Button>
        </div>
        <p className={styles.hint}>
          The log file helps track down problems. It never contains passwords or project contents. The templates
          folder holds the templates you made (for support; change them in Settings → Templates).
        </p>
      </section>
    </>
  );
}
