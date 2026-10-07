import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Menu } from "@tauri-apps/api/menu";
import PreviewTree from "../../components/PreviewTree";
import { Button } from "../../components/ui";
import {
  deleteTemplate,
  exportTemplate,
  importTemplate,
  listTemplates,
  previewTemplate,
  resetTemplate,
  setTemplateHidden,
  templateDraft,
} from "../../lib/commands";
import { isMac } from "../../lib/platform";
import { asPlan } from "../../lib/templateDraft";
import type { SaveMode, Template, TemplateDraft, TemplateInfo, TemplatePreview } from "../../lib/types";
import type { TabProps } from "../Settings";
import TemplateEditor, { type EditorStep } from "./editor/TemplateEditor";
import styles from "./settings.module.css";

interface Editing {
  sourceKey: string;
  mode: SaveMode;
  start: Template;
  starterFiles: string[];
  step: EditorStep;
}

/** Templates: what each one makes, Customize / Edit in the app, new ones, sharing between
 *  computers, and taking yours away (to the Recycle Bin / Trash). */
export default function TemplatesTab({ run }: TabProps) {
  const [templates, setTemplates] = useState<TemplateInfo[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  // Loaded for one key; shown only while that key is selected (no stale details).
  const [loaded, setLoaded] = useState<{ key: string; draft: TemplateDraft; preview: TemplatePreview | null } | null>(null);
  const draft = loaded?.key === selected ? loaded.draft : null;
  const preview = loaded?.key === selected ? loaded.preview : null;
  const [editing, setEditing] = useState<Editing | null>(null);
  const bin = isMac() ? "Trash" : "Recycle Bin";

  const load = useCallback(
    () =>
      listTemplates()
        .then((all) => {
          setTemplates(all);
          setSelected((s) => (s && all.some((t) => t.key === s) ? s : (all.find((t) => !t.superseded)?.key ?? null)));
        })
        .catch(() => {}),
    [],
  );
  useEffect(() => {
    void load();
    let unlisten: (() => void) | undefined;
    listen("settings-changed", () => void load())
      .then((u) => (unlisten = u))
      .catch(() => {});
    return () => unlisten?.();
  }, [load]);

  // What the selected template makes.
  useEffect(() => {
    if (!selected) return;
    let live = true;
    templateDraft(selected)
      .then((d) =>
        previewTemplate(selected, d.template)
          .then((p) => live && setLoaded({ key: selected, draft: d, preview: p }))
          .catch(() => live && setLoaded({ key: selected, draft: d, preview: null })),
      )
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [selected, templates]);

  const current = templates.find((t) => t.key === selected) ?? null;
  const yoursFor = (id: string) => templates.find((t) => !t.builtIn && !t.superseded && t.id === id);
  const replacesBuiltIn = (t: TemplateInfo) => !t.builtIn && templates.some((b) => b.builtIn && b.replaced && b.id === t.id);
  const listed = useMemo(
    // One copy per template (T-A): older copies left over after a failed bin move stay hidden.
    () => templates.filter((t) => !t.superseded || t.replaced),
    [templates],
  );

  const open = async (mode: SaveMode, key: string, step: EditorStep = "questions", empty = false) => {
    const d = await run(templateDraft(key));
    if (!d) return;
    let start = d.template;
    if (mode === "new") {
      start = empty
        ? { ...start, name: "New template", description: "", tree: [] }
        : { ...start, name: `${start.name} (copy)` };
    }
    setEditing({ sourceKey: key, mode, start, starterFiles: d.starterFiles, step });
  };

  const newMenu = async () => {
    const blank = templates.find((t) => t.builtIn && t.id === "blank") ?? templates.find((t) => !t.superseded);
    try {
      // Start empty, or from a copy of any template you have (e.g. Grading: MV → a short-film one).
      const copies = templates.filter((t) => !t.superseded && !t.replaced && t.id !== "blank");
      const menu = await Menu.new({
        items: [
          {
            id: "empty",
            text: "Start empty",
            enabled: !!blank,
            action: () => blank && void open("new", blank.key, "questions", true),
          },
          ...(copies.length > 0
            ? [
                { item: "Separator" as const },
                { id: "copy-label", text: "Start from a copy of:", enabled: false },
              ]
            : []),
          ...copies.map((t) => ({
            id: `copy:${t.key}`,
            // On Windows "&" marks a menu shortcut letter; "&&" shows one "&".
            text: `    ${isMac() ? t.name : t.name.replace(/&/g, "&&")}`,
            action: () => void open("new", t.key),
          })),
        ],
      });
      await menu.popup();
    } catch {
      /* the menu couldn't open: nothing to do */
    }
  };

  const changeFolders = () => {
    if (!current) return;
    if (current.builtIn) {
      const yours = yoursFor(current.id);
      if (yours) setSelected(yours.key);
      else void open("customize", current.key, "folders");
    } else void open("edit", current.key, "folders");
  };

  return (
    <>
      <p className={styles.hint}>
        A template is the set of folders and files Nest makes for a new project, plus the questions it asks (who it's
        billed to, project name, start date…). You pick one each time you click New Project. To change one, select it
        and click Customize.
      </p>
      <div className={styles.actions}>
        <Button variant="primary" onClick={() => void newMenu()}>
          New template…
        </Button>
        <Button
          onClick={async () => {
            const t = await run(importTemplate());
            if (t) {
              setSelected(t.key);
              void run(Promise.resolve(null), `Added “${t.name}”.`);
            }
          }}
        >
          Add from a file…
        </Button>
      </div>

      <div className={`${styles.card} ${styles.tlist}`} role="listbox" aria-label="Templates">
        {listed.map((t) => (
          <button
            key={t.key}
            type="button"
            role="option"
            aria-selected={t.key === selected}
            className={styles.titem}
            onClick={() => setSelected(t.key)}
          >
            <span className={styles.tname}>{t.name}</span>
            <span className={styles.tdesc}>
              {t.replaced
                ? "Nest's original (you use your own version)"
                : t.description}
            </span>
            <span className={t.builtIn ? styles.pillMuted : styles.pill}>
              {t.builtIn ? (t.replaced ? "Original" : "Built in") : replacesBuiltIn(t) ? "Customized" : "Yours"}
            </span>
            {t.hidden && !t.replaced && <span className={styles.tflag}>Not in list</span>}
          </button>
        ))}
      </div>

      {current && (
        <section key={current.key} className={`${styles.card} ${styles.detail}`} aria-label={current.name}>
          <div className={styles.detailHead}>
            <h3 className={styles.detailTitle}>{current.name}</h3>
            <label className={styles.check}>
              <input
                type="checkbox"
                checked={!current.hidden && !current.replaced && !current.superseded}
                disabled={current.replaced || current.superseded}
                onChange={(e) => void run(setTemplateHidden(current.id, !e.target.checked))}
              />
              Show in the New Project list
            </label>
          </div>
          <p className={styles.hint}>
            {current.replaced
              ? "You're using your own version of this template. This is Nest's original, kept so you can go back."
              : current.superseded
                ? "An older version, kept on disk. New Project uses the newest one."
                : current.hidden
                  ? "Hidden from the New Project list. Nothing is deleted; tick the box to show it again."
                  : "You'll see it when you click New Project. Untick if you never use it; nothing is deleted."}
          </p>

          {preview && (
            <>
              <div className={styles.section}>
                <h4 className={styles.sectionTitle}>Project folder name</h4>
                <div className={`${styles.example} ${styles.mono}`}>{preview.folderName}</div>
              </div>
              {draft && (
                <div className={styles.section}>
                  <h4 className={styles.sectionTitle}>Asks you for</h4>
                  <div className={styles.qs}>
                    {draft.template.fields.map((f) => (
                      <span key={f.key} className={styles.q}>
                        {f.type === "client" ? "Billed to" : f.label}
                      </span>
                    ))}
                  </div>
                </div>
              )}
              <div className={styles.section}>
                <div className={styles.rowBetween}>
                  <h4 className={styles.sectionTitle}>Makes these folders</h4>
                  {!current.replaced && (
                    <Button className={styles.linkButton} onClick={changeFolders}>
                      Change folders…
                    </Button>
                  )}
                </div>
                <PreviewTree plan={asPlan(preview)} />
              </div>
            </>
          )}

          <div className={styles.actions}>
            {current.builtIn && !current.replaced && (
              <Button variant="primary" onClick={() => void open("customize", current.key)}>
                Customize…
              </Button>
            )}
            {current.replaced && (
              <Button variant="primary" onClick={() => setSelected(yoursFor(current.id)?.key ?? null)}>
                Open your version
              </Button>
            )}
            {!current.builtIn && (
              <Button variant="primary" onClick={() => void open("edit", current.key)}>
                Edit…
              </Button>
            )}
            <Button onClick={() => void open("new", current.key)}>Duplicate</Button>
            <Button
              title="Saves this template as one file you can open on another computer with “Add from a file…”"
              onClick={async () => {
                if (await run(exportTemplate(current.key)))
                  void run(
                    Promise.resolve(null),
                    `Saved “${current.name}” as a file. On your other computer: Settings → Templates → Add from a file…`,
                  );
              }}
            >
              Send to another computer…
            </Button>
            {!current.builtIn && replacesBuiltIn(current) && (
              <Button
                onClick={async () => {
                  if (await run(resetTemplate(current.key)))
                    void run(Promise.resolve(null), `Back to Nest's original “${current.name}”. Yours is in the ${bin}.`);
                }}
              >
                Go back to Nest's original…
              </Button>
            )}
            {!current.builtIn && !replacesBuiltIn(current) && (
              <Button
                onClick={async () => {
                  if (await run(deleteTemplate(current.key)))
                    void run(Promise.resolve(null), `Moved “${current.name}” to the ${bin}.`);
                }}
              >
                Move to {bin}…
              </Button>
            )}
          </div>
        </section>
      )}

      {editing && (
        <TemplateEditor
          {...editing}
          onClose={() => setEditing(null)}
          onSaved={({ info, binned, binProblem }) => {
            setEditing(null);
            setSelected(info.key);
            void load();
            const first =
              editing.mode === "customize"
                ? `Saved. New Project now uses your version of “${info.name}”. Nest's original is kept.`
                : `Saved “${info.name}”.`;
            // Decision T-A: one copy per template; say where the previous one went.
            const previous = binProblem
              ? ` ${binProblem}`
              : binned > 0
                ? ` The previous version is in the ${bin}.`
                : "";
            void run(Promise.resolve(null), first + previous);
          }}
        />
      )}
    </>
  );
}
