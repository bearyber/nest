import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Menu } from "@tauri-apps/api/menu";
import { Button, IconButton } from "../../components/ui";
import {
  editorClose,
  editorOpen,
  exportTemplate,
  importTemplate,
  listTemplates,
  removeTemplate,
  restoreBuiltinTemplates,
  setTemplateHidden,
  templateFromFolder,
} from "../../lib/commands";
import { isMac } from "../../lib/platform";
import type { FolderDraft, SaveMode, Skipped, SkipReason, Template, TemplateInfo } from "../../lib/types";
import TemplateEditor from "./TemplateEditor";
import styles from "./Templates.module.css";

interface Editing {
  sourceKey: string;
  mode: SaveMode;
  start: Template;
  note?: string;
}

/** What a folder left out, in plain words. System files aren't mentioned: nobody misses them. */
const LEFT_OUT: Record<Exclude<SkipReason, "clutter">, [string, string]> = {
  tooBig: ["file over 100 MB", "files over 100 MB"],
  cache: ["cache, auto-save or app library folder", "cache, auto-save and app library folders"],
  link: ["shortcut", "shortcuts"],
  unreadable: ["item Nest couldn't read", "items Nest couldn't read"],
  badName: ["name that can't work on both Windows and Mac", "names that can't work on both Windows and Mac"],
};

export function folderNote(skipped: Skipped[], fromProject: boolean): string {
  const parts = (Object.keys(LEFT_OUT) as (keyof typeof LEFT_OUT)[]).flatMap((reason) => {
    const n = skipped.filter((s) => s.reason === reason).length;
    return n ? [`${n} ${LEFT_OUT[reason][n === 1 ? 0 : 1]}`] : [];
  });
  const answers = fromProject ? "This was a Nest project: its answers became questions New Project asks. " : "";
  const left = parts.length ? `Left out: ${parts.join(", ")}.` : "Everything came along.";
  return `${answers}${left} Your folder wasn't changed.`;
}

/** On Windows "&" marks a menu shortcut letter; "&&" shows one "&". */
const menuText = (s: string) => (isMac() ? s : s.replace(/&/g, "&&"));

/** Templates (main window → sidebar → Templates): drop a job folder to make one, click one to
 *  change it, right-click (or ⋯) for the rest. */
interface Props {
  /** Run a command: shows its error, or `ok` when it worked. */
  run: <T>(p: Promise<T>, ok?: string) => Promise<T | undefined>;
  /** New Project, starting with this template. */
  onNewProject?: (templateId: string) => void;
}

export default function Templates({ run, onNewProject }: Props) {
  const [templates, setTemplates] = useState<TemplateInfo[]>([]);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [dragging, setDragging] = useState(false);
  const bin = isMac() ? "Trash" : "Recycle Bin";

  const load = useCallback(
    () =>
      listTemplates()
        .then(setTemplates)
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

  // One card per template: yours, and Nest's that you haven't replaced (your version is listed
  // instead) or removed. Older copies of yours (T-A) stay out of the list.
  const listed = useMemo(() => templates.filter((t) => !t.superseded && !(t.builtIn && t.hidden)), [templates]);
  const nestsHidden = templates.some((t) => t.builtIn && t.hidden);

  const open = async (mode: SaveMode, key: string, empty = false) => {
    const opened = await run(editorOpen(key));
    if (!opened) return;
    let start = opened.draft.template;
    if (mode === "new") {
      start = empty
        ? { ...start, name: "New template", description: "", tree: [] }
        : { ...start, name: `${start.name} (copy)` };
    }
    setEditing({ sourceKey: opened.key, mode, start });
  };

  const fromFolder = async (path: string | null) => {
    if (editing) return; // a drop while the editor is open isn't meant for a new template
    const made: FolderDraft | null | undefined = await run(templateFromFolder(path));
    if (!made) return;
    setEditing({
      sourceKey: made.key,
      mode: "new",
      start: made.draft.template,
      note: folderNote(made.skipped, made.fromProject),
    });
  };

  // Drop a folder anywhere on this view to make a template from it.
  const fromFolderRef = useRef(fromFolder);
  const editingRef = useRef(false);
  useEffect(() => {
    fromFolderRef.current = fromFolder;
    editingRef.current = editing !== null;
  });
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "drop") {
          setDragging(false);
          if (p.paths.length > 0) void fromFolderRef.current(p.paths[0]);
        } else if (p.type === "leave") setDragging(false);
        else if (!editingRef.current) setDragging(true);
      })
      .then((u) => (unlisten = u))
      .catch(() => {});
    return () => unlisten?.();
  }, []);

  const changeTemplate = (t: TemplateInfo) => void open(t.builtIn ? "customize" : "edit", t.key);

  const remove = async (t: TemplateInfo) => {
    const done = await run(removeTemplate(t.key));
    if (!done) return;
    void run(
      Promise.resolve(null),
      t.builtIn ? `Removed “${t.name}”. “Bring back Nest's templates” shows it again.` : `Moved “${t.name}” to the ${bin}.`,
    );
  };

  const templateMenu = async (t: TemplateInfo) => {
    try {
      const menu = await Menu.new({
        items: [
          {
            id: "new-project",
            text: "New project from this",
            enabled: !!onNewProject && !t.hidden,
            action: () => onNewProject?.(t.id),
          },
          { id: "duplicate", text: "Duplicate", action: () => void open("new", t.key) },
          {
            id: "export",
            text: "Export…",
            action: async () => {
              if (await run(exportTemplate(t.key)))
                void run(Promise.resolve(null), `Saved “${t.name}” as a file. On your other computer: Templates → Import…`);
            },
          },
          // Older Nest versions could hide your own templates: let them come back.
          ...(!t.builtIn && t.hidden
            ? [
                {
                  id: "show",
                  text: "Show in New Project",
                  action: () => void run(setTemplateHidden(t.id, false)),
                },
              ]
            : []),
          { item: "Separator" as const },
          { id: "remove", text: t.builtIn ? "Remove" : "Remove…", action: () => void remove(t) },
        ],
      });
      await menu.popup();
    } catch {
      /* the menu couldn't open: nothing to do */
    }
  };

  const newMenu = async () => {
    const blank = templates.find((t) => t.builtIn && t.id === "blank") ?? listed[0];
    try {
      // Start empty, or from a copy of any template you have (e.g. Grading: MV → a short-film one).
      const copies = listed.filter((t) => t.id !== "blank");
      const menu = await Menu.new({
        items: [
          {
            id: "empty",
            text: "Start empty",
            enabled: !!blank,
            action: () => blank && void open("new", blank.key, true),
          },
          {
            id: "copy",
            text: "From a copy of…",
            enabled: copies.length > 0,
            items: copies.map((t) => ({
              id: `copy:${t.key}`,
              text: menuText(t.name),
              action: () => void open("new", t.key),
            })),
          },
        ],
      });
      await menu.popup();
    } catch {
      /* the menu couldn't open: nothing to do */
    }
  };

  return (
    <>
      <div className={styles.head}>
        <Button variant="primary" onClick={() => void newMenu()}>
          New template
        </Button>
        <Button
          title="Opens a .nesttemplate file made with Export… (on this or another computer)"
          onClick={async () => {
            const t = await run(importTemplate());
            if (t) void run(Promise.resolve(null), `Added “${t.name}”.`);
          }}
        >
          Import…
        </Button>
      </div>

      <div className={styles.drop} data-dragging={dragging || undefined}>
        <p className={styles.dropText}>Drop a job folder here to make a template from it</p>
        <Button onClick={() => void fromFolder(null)}>Choose a folder…</Button>
      </div>

      <ul className={styles.list} aria-label="Templates">
        {listed.map((t) => (
          <li
            key={t.key}
            className={styles.card}
            onContextMenu={(e) => {
              e.preventDefault();
              void templateMenu(t);
            }}
          >
            <button type="button" className={styles.open} onClick={() => changeTemplate(t)}>
              <span className={styles.name}>{t.name}</span>
              <span className={styles.meta}>
                {t.builtIn ? "Nest's" : "Yours"} · {t.folders} folder{t.folders === 1 ? "" : "s"}
                {!t.builtIn && t.hidden ? " · Not in New Project" : ""}
              </span>
            </button>
            <IconButton icon="more" label={`More for ${t.name}`} onClick={() => void templateMenu(t)} />
          </li>
        ))}
      </ul>

      {nestsHidden && (
        <Button
          className={styles.link}
          onClick={() => void run(restoreBuiltinTemplates(), "Nest's templates are back.")}
        >
          Bring back Nest's templates
        </Button>
      )}

      {editing && (
        <TemplateEditor
          {...editing}
          onClose={() => {
            void editorClose(editing.sourceKey).catch(() => {});
            setEditing(null);
          }}
          onSaved={({ info, binned, binProblem }) => {
            setEditing(null);
            void load();
            const first =
              editing.mode === "customize" ? "Saved as yours. Nest's original is kept." : `Saved “${info.name}”.`;
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
