import { useEffect, useState } from "react";
import Icon from "../../components/Icon";
import { Button, PathText } from "../../components/ui";
import { chooseClientKit, chooseKitsFolder, clearClientKit, clearKitsFolder, kitsView } from "../../lib/commands";
import { errorMessage, type KitClient, type KitsView } from "../../lib/types";
import type { TabProps } from "../Settings";
import styles from "./settings.module.css";

/** "Kits/Apple" → shown as the folder's own name, the full path on hover. */
const lastPart = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

/** Client kits (v0.5): each client's brand folder, copied into new projects for that client.
 *  Per computer: nothing here is shared with your other computers. */
export default function KitsTab({ view, run }: TabProps) {
  const [kits, setKits] = useState<KitsView | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  // Re-read whenever the settings change (here or in another window).
  useEffect(() => {
    let live = true;
    kitsView()
      .then((k) => live && setKits(k))
      .catch((e) => live && setProblem(errorMessage(e)));
    return () => {
      live = false;
    };
  }, [view]);

  const folder = view.settings.kitsFolder;

  const status = (c: KitClient) => {
    if (!c.kit) return <span className={styles.sub}>No files yet</span>;
    if (c.missing) return <span className={styles.sub}>Can't find {c.kit}</span>;
    return (
      <span className={`${styles.sub} ${styles.mono}`} title={c.kit}>
        {c.source === "chosen" ? <PathText path={c.kit} /> : `Clients folder → ${lastPart(c.kit)}`}
      </span>
    );
  };

  return (
    <>
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Folder with all your clients</h3>
        <p className={styles.hint}>
          Optional: a folder with one folder per client, named after the client's code or name (for example
          Clients/APL or Clients/Apple). New Project copies that client's files into the new project. Nothing in it is
          ever changed. You can also pick a client's folder right in New Project.
        </p>
        {folder ? (
          <div className={styles.card}>
            <div className={styles.row}>
              <Icon name="folder" className={styles.folderIcon} />
              <span className={`${styles.grow} ${styles.mono}`}>
                <PathText path={folder} />
              </span>
              {kits?.kitsFolderMissing && <span className={styles.pillMuted}>Not found</span>}
              <Button onClick={() => run(chooseKitsFolder())}>Change…</Button>
              <Button onClick={() => run(clearKitsFolder(), "Nest stopped using that folder. Nothing on disk changed.")}>
                Stop using
              </Button>
            </div>
          </div>
        ) : (
          <div className={styles.actions}>
            <Button onClick={() => run(chooseKitsFolder())}>Choose a folder…</Button>
          </div>
        )}
      </section>

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Clients</h3>
        <p className={styles.hint}>
          A folder chosen for one client wins over the folder above. Only on this computer.
        </p>
        {problem && <p className={styles.hint}>{problem}</p>}
        {kits && kits.clients.length === 0 && (
          <p className={styles.hint}>Clients show here once you have projects for them.</p>
        )}
        {kits && kits.clients.length > 0 && (
          <div className={styles.card}>
            {kits.clients.map((c) => (
              <div key={c.code} className={styles.row}>
                <span className={styles.grow}>
                  <span className={styles.name}>{c.name}</span> <span className={styles.sub}>{c.code}</span>
                  <br />
                  {status(c)}
                </span>
                <Button onClick={() => run(chooseClientKit(c.code, c.name))}>Choose a folder…</Button>
                {c.source === "chosen" && (
                  <Button
                    onClick={() =>
                      run(
                        clearClientKit(c.code),
                        folder ? `${c.name} uses the clients folder again.` : `${c.name} has no files folder now.`,
                      )
                    }
                  >
                    Use the clients folder
                  </Button>
                )}
              </div>
            ))}
          </div>
        )}
      </section>
    </>
  );
}
