import { useCallback, useEffect, useRef, useState } from "react";
import { Menu } from "@tauri-apps/api/menu";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Button } from "../../components/ui";
import {
  editorAddFiles,
  editorAddResolveDrp,
  editorAddTextFile,
  exampleNames,
  previewTemplate,
  saveTemplate,
} from "../../lib/commands";
import {
  addFolder,
  formatName,
  indent,
  isBlank,
  moveField,
  outdent,
  parentOf,
  parseName,
  record,
  redo,
  removeFolder,
  startHistory,
  undo,
  withoutFilesIn,
  type History,
} from "../../lib/editorModel";
import {
  blankField,
  canHoldKit,
  cloneNodes,
  followMoves,
  fromNodes,
  locate,
  moveNode,
  newKey,
  nodeId,
  removeQuestion,
  setKitFolder,
  toNodes,
  type FolderNode,
} from "../../lib/templateDraft";
import {
  errorMessage,
  type AddedFiles,
  type Field,
  type ResolveStarter,
  type SaveMode,
  type Saved,
  type Template,
  type TemplatePreview,
} from "../../lib/types";
import { EDIT_STEP_EVENT, type EditStep } from "../../lib/editMenu";
import { useDialogFocus } from "../../lib/useDialogFocus";
import { usePointerDrag } from "./drag";
import { blankLabel } from "./NameField";
import QuestionSettings from "./QuestionSettings";
import ResolveSection, { defaultFolder, drpName, isDrp } from "./ResolveSection";
import Tree, { fileRow, ROOT, rowOrder, visibleNames, type RowId } from "./Tree";
import styles from "./editor.module.css";

/** Everything Undo can step back through. */
interface Doc {
  draft: Template;
  nodes: FolderNode[];
}

interface Props {
  /** The editor session (Rust keeps where the draft came from and files you add). */
  sourceKey: string;
  mode: SaveMode;
  start: Template;
  /** Shown in the footer instead of the usual hint (e.g. what a folder left out). */
  note?: string;
  onClose: () => void;
  onSaved: (saved: Saved) => void;
}

const NEW_FOLDER = "New folder";
const isTyping = (t: EventTarget | null) => !!(t as HTMLElement | null)?.closest?.("input, textarea, select");
const isMac = () => navigator.userAgent.includes("Mac");

/** The template editor (PLAN 2026-10-13): one screen. The tree is the template, every name shown
 *  as an example project would have it; click a name to change it, and New Project's answers go
 *  in as chips. Right-click (or ⋯) for the rest. Rust previews every change with the real
 *  planner and does the saving. */
export default function TemplateEditor({ sourceKey, mode, start, note, onClose, onSaved }: Props) {
  const sheetRef = useRef<HTMLDivElement>(null);
  // Focus starts on the editor itself (so Escape still closes it), stays inside, and goes back.
  useDialogFocus(sheetRef, { initialFocus: "container" });
  const [hist, setHist] = useState<History<Doc>>(() => startHistory({ draft: start, nodes: toNodes(start.tree) }));
  const { draft, nodes } = hist.present;
  const [selected, setSelected] = useState<RowId | null>(null);
  const [renaming, setRenaming] = useState<RowId | null>(null);
  const [newRow, setNewRow] = useState<RowId | null>(null);
  const [blank, setBlank] = useState<{ key: string; row?: RowId; index?: number } | null>(null);
  const [showMore, setShowMore] = useState(false);
  const [dropPath, setDropPath] = useState<string | null>(null);
  const [preview, setPreview] = useState<TemplatePreview | null>(null);
  const [previewFor, setPreviewFor] = useState<string | null>(null);
  const [examples, setExamples] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<{ text: string; undo: boolean } | null>(null);
  const [confirmClose, setConfirmClose] = useState(false);
  /** A dropped .drp being added (its name), and whether a drag over the editor is .drp only. */
  const [addingDrp, setAddingDrp] = useState<string | null>(null);
  const [draggingDrp, setDraggingDrp] = useState(false);
  const dirty = useRef(false);
  const treeRef = useRef<HTMLDivElement>(null);

  const labelOf = useCallback((key: string) => draft.fields.find((f) => f.key === key)?.label ?? key, [draft.fields]);
  const tree = fromNodes(nodes, labelOf);
  const full: Template = { ...draft, tree: tree.tree };
  const files = draft.files ?? [];

  /** One undoable change. Always from the latest state, so quick changes in a row all count. */
  const change = useCallback((fn: (d: Doc) => Doc) => {
    dirty.current = true;
    setHist((h) => record(h, fn(h.present)));
  }, []);
  const say = (text: string, canUndo = false) => setToast({ text, undo: canUndo });
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 6000);
    return () => clearTimeout(t);
  }, [toast]);

  // Ask Rust what this draft would make (debounced). Only the reply to the latest draft counts,
  // and Save waits until it has arrived.
  // With it, every name the tree shows as the example project has it (one call for all of them).
  const fullJson = JSON.stringify(full);
  const namesJson = JSON.stringify(visibleNames(draft.folderName, nodes, files));
  useEffect(() => {
    let live = true;
    const t = setTimeout(() => {
      const d = JSON.parse(fullJson) as Template;
      previewTemplate(sourceKey, d)
        .then((p) => {
          if (!live) return;
          setPreview(p);
          setPreviewFor(fullJson);
          setError(null);
        })
        .catch((e) => live && setError(errorMessage(e)));
      const names = JSON.parse(namesJson) as string[];
      exampleNames(sourceKey, d, names)
        .then((out) => {
          if (!live) return;
          const next: Record<string, string> = {};
          names.forEach((n, i) => {
            if (typeof out[i] === "string") next[n] = out[i];
          });
          setExamples(next);
        })
        // Names then show as typed; the preview reports anything that's really wrong.
        .catch((e) => console.warn("Example names didn't come back", e));
    }, 200);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [fullJson, namesJson, sourceKey]);
  const fresh = previewFor === fullJson;
  const problems = [...tree.problems, ...(preview?.problems ?? [])];

  // ── Changes ──

  const pathOf = (d: Doc, id: RowId) => (id === ROOT ? "" : (locate(d.nodes, id)?.path ?? null));
  /** Folders moved or renamed: files inside them follow. */
  const withNodes = (d: Doc, next: FolderNode[]): Doc => ({
    nodes: next,
    draft: { ...d.draft, files: followMoves(d.draft.files ?? [], d.nodes, next) },
  });

  const startRename = (id: RowId, isNew = false) => {
    setSelected(id);
    setRenaming(id);
    setNewRow(isNew ? id : null);
  };

  const addInside = (parent: string | null) => {
    let made: string | null = null;
    change((d) => {
      const r = addFolder(d.nodes, parent, NEW_FOLDER);
      made = r.id;
      return { ...d, nodes: r.nodes };
    });
    // The new folder's name is ready to type over.
    setTimeout(() => made && startRename(made, true), 0);
  };

  const rename = (id: RowId, value: string, then?: "below" | "in" | "out") => {
    setRenaming(null);
    const v = value.trim();
    if (id === ROOT) {
      if (v) change((d) => ({ ...d, draft: { ...d.draft, folderName: v } }));
    } else if (id.startsWith("file:")) {
      const from = id.slice(5);
      if (v)
        change((d) => ({
          ...d,
          draft: {
            ...d.draft,
            files: (d.draft.files ?? []).map((f) => {
              if (f.from !== from) return f;
              const slash = f.to.lastIndexOf("/");
              return { ...f, to: slash < 0 ? v : `${f.to.slice(0, slash)}/${v}` };
            }),
          },
        }));
    } else {
      change((d) => {
        let next = d.nodes;
        if (v) {
          next = cloneNodes(d.nodes);
          const at = locate(next, id);
          if (at) at.node.name = v;
        }
        if (then === "in") next = indent(next, id) ?? next;
        if (then === "out") next = outdent(next, id) ?? next;
        return withNodes(d, next);
      });
      if (then === "below") {
        let made: string | null = null;
        change((d) => {
          const r = addFolder(d.nodes, parentOf(d.nodes, id), NEW_FOLDER, id);
          made = r.id;
          return { ...d, nodes: r.nodes };
        });
        setTimeout(() => made && startRename(made, true), 0);
      } else if (then) setTimeout(() => startRename(id), 0);
      else treeRef.current?.focus();
    }
  };

  const remove = (id: RowId) => {
    if (id === ROOT) return;
    if (id.startsWith("file:")) {
      const from = id.slice(5);
      const name = files.find((f) => f.from === from)?.to ?? from;
      change((d) => ({ ...d, draft: { ...d.draft, files: (d.draft.files ?? []).filter((f) => f.from !== from) } }));
      say(`Removed ${name}.`, true);
    } else {
      const at = locate(nodes, id);
      if (!at) return;
      change((d) => {
        const path = pathOf(d, id) ?? at.path;
        return {
          nodes: removeFolder(d.nodes, id),
          draft: { ...d.draft, files: withoutFilesIn(d.draft.files ?? [], path) },
        };
      });
      say(`Deleted ${at.node.name}.`, true);
    }
    setSelected(null);
  };

  const move = (id: string, target: string, where: "before" | "after" | "inside") =>
    change((d) => withNodes(d, moveNode(d.nodes, id, target, where)));

  const shift = (id: RowId, out: boolean) =>
    change((d) => {
      const next = out ? outdent(d.nodes, id) : indent(d.nodes, id);
      return next ? withNodes(d, next) : d;
    });

  const setWhen = (id: string, when: string | null) =>
    change((d) => {
      const next = cloneNodes(d.nodes);
      const at = locate(next, id);
      if (!at) return d;
      if (when) at.node.when = when;
      else delete at.node.when;
      at.node.explicit = true;
      return { ...d, nodes: next };
    });

  /** The client's kit (Settings → Client kits) is copied into this folder on Create. One folder
   *  per template: putting it somewhere else moves it. `null` = nowhere. */
  const setKit = (id: string | null) => {
    change((d) => ({ ...d, nodes: setKitFolder(d.nodes, id) }));
    if (id) say(`New projects get the client's kit in ${locate(nodes, id)?.path ?? "this folder"}.`, true);
    else say("New projects from this template won't get the client's files.", true);
  };

  /** "One folder for each…": a `{item}` folder inside this one, made once per picked choice. */
  const setEach = (id: string, key: string | null) =>
    change((d) => {
      const next = cloneNodes(d.nodes);
      const at = locate(next, id);
      if (!at) return d;
      const existing = at.node.kids.find((k) => k.each);
      if (key && existing) existing.each = key;
      else if (key) at.node.kids.push({ id: nodeId(), name: "{item}", each: key, explicit: true, kids: [] });
      else at.node.kids = at.node.kids.filter((k) => !k.each);
      at.node.explicit = true;
      const prefix = existing ? `${at.path}/${existing.name}` : null;
      return {
        nodes: next,
        draft: { ...d.draft, files: !key && prefix ? withoutFilesIn(d.draft.files ?? [], prefix) : (d.draft.files ?? []) },
      };
    });

  // ── Questions (what New Project asks; their answers go into names) ──

  /** A new question New Project asks, named `label`. Returns its key. */
  const newBlank = (label: string, kind: Field["type"] = /date/i.test(label) ? "date" : "text"): string => {
    const name = label.trim() || "New question";
    const key = newKey(
      name,
      draft.fields.map((f) => f.key),
    );
    const field = { ...blankField(kind, [], name), key };
    change((d) => ({ ...d, draft: { ...d.draft, fields: [...d.draft.fields, field] } }));
    return key;
  };

  const updateField = (f: Field) =>
    change((d) => ({ ...d, draft: { ...d.draft, fields: d.draft.fields.map((x) => (x.key === f.key ? f : x)) } }));

  const removeBlank = (key: string) => {
    const label = labelOf(key);
    change((d) => {
      const t = removeQuestion({ ...d.draft, tree: fromNodes(d.nodes).tree }, key);
      return { draft: t, nodes: toNodes(t.tree) };
    });
    setBlank(null);
    say(`Removed the question “${label}” and took its answer out of the names.`, true);
  };

  /** The name a chip sits in, and a way to change it. */
  const nameAt = (row: RowId): { get: (d: Doc) => string; set: (d: Doc, v: string) => Doc } | null => {
    if (row === ROOT) return { get: (d) => d.draft.folderName, set: (d, v) => ({ ...d, draft: { ...d.draft, folderName: v } }) };
    if (row.startsWith("file:")) {
      const from = row.slice(5);
      return {
        get: (d) => (d.draft.files ?? []).find((f) => f.from === from)?.to ?? "",
        set: (d, v) => ({ ...d, draft: { ...d.draft, files: (d.draft.files ?? []).map((f) => (f.from === from ? { ...f, to: v } : f)) } }),
      };
    }
    return {
      get: (d) => locate(d.nodes, row)?.node.name ?? "",
      set: (d, v) => {
        const next = cloneNodes(d.nodes);
        const at = locate(next, row);
        if (at) at.node.name = v;
        return withNodes(d, next);
      },
    };
  };

  const chipTransform = (() => {
    if (!blank?.row || blank.index === undefined || blank.index < 0) return undefined;
    const at = nameAt(blank.row);
    if (!at) return undefined;
    const pieces = parseName(at.get(hist.present));
    // For a file, the chip index is in its file name; the full path's pieces are offset.
    const piece = blank.row.startsWith("file:")
      ? parseName(at.get(hist.present).slice(at.get(hist.present).lastIndexOf("/") + 1))[blank.index]
      : pieces[blank.index];
    if (!piece || !isBlank(piece)) return undefined;
    return {
      transform: piece.transform,
      onChange: (t: string | undefined) =>
        change((d) => {
          const name = at.get(d);
          const slash = blank.row!.startsWith("file:") ? name.lastIndexOf("/") + 1 : 0;
          const ps = parseName(name.slice(slash));
          const p = ps[blank.index!];
          if (!p || !isBlank(p)) return d;
          ps[blank.index!] = t ? { key: p.key, transform: t } : { key: p.key };
          return at.set(d, name.slice(0, slash) + formatName(ps));
        }),
    };
  })();

  // ── Files from Finder / Explorer ──

  /** Put files Rust staged into the draft (with any folders they came in). */
  const takeAdded = (added: AddedFiles) =>
    change((d) => ({
      nodes: added.folders.length ? toNodes([...fromNodes(d.nodes).tree, ...added.folders]) : d.nodes,
      draft: { ...d.draft, files: [...(d.draft.files ?? []), ...added.files] },
    }));

  /** "Project info": a short text file with the template's own answers filled in on Create. */
  const addProjectInfo = async (into: string) => {
    const lines = ["Job code: {jobCode}"];
    for (const f of draft.fields) {
      if (f.type === "client") lines.push(`Client: {${f.key}}`);
      else if (f.type === "text" || f.type === "date" || f.type === "select") lines.push(`${f.label}: {${f.key}}`);
    }
    for (const f of draft.fields.filter((x) => x.type === "longtext")) lines.push("", `${f.label}:`, `{${f.key}}`);
    lines.push("", "Made with Nest on {date} from the \u201c{templateName}\u201d template.", "");
    try {
      const added = await editorAddTextFile(sourceKey, into, "Project info.txt", lines.join("\n"));
      takeAdded(added);
      say("Added Project info.txt. New projects get it with their answers filled in.", true);
    } catch (e) {
      say(errorMessage(e));
    }
  };

  /** "+ Add a file": your own, or the one ready-made file. */
  const addFileMenu = async (into: string) => {
    try {
      const m = await Menu.new({
        items: [
          { text: "Your own file…", action: () => void addFiles(into, null) },
          { text: "Project info (job code, client, project, dates)", action: () => void addProjectInfo(into) },
          { item: "Separator" },
          { text: "Tip: you can also drag files here from Finder or Explorer", enabled: false },
        ],
      });
      await m.popup();
    } catch (e) {
      console.warn("Add a file: menu didn't open", e);
      void addFiles(into, null);
    }
  };

  const addFiles = async (into: string, paths: string[] | null) => {
    try {
      const added = await editorAddFiles(sourceKey, into, paths);
      if (!added) return;
      change((d) => ({
        nodes: added.folders.length ? toNodes([...fromNodes(d.nodes).tree, ...added.folders]) : d.nodes,
        draft: { ...d.draft, files: [...(d.draft.files ?? []), ...added.files] },
      }));
      const left = added.skipped.filter((s) => s.reason !== "clutter").length;
      const n = added.files.length;
      say(
        `Added ${n} file${n === 1 ? "" : "s"}.${left ? ` Left out ${left} (footage over 100 MB, caches or names that can't work on both Windows and Mac).` : ""}`,
        n > 0,
      );
    } catch (e) {
      say(errorMessage(e));
    }
  };

  // ── The Resolve project ──

  /** Set or take away the template's Resolve project (one undoable change). */
  const setResolve = (next: ResolveStarter | undefined) =>
    change((d) => {
      const draft = { ...d.draft, resolve: next };
      if (!next) delete draft.resolve;
      return { ...d, draft };
    });

  const removeResolve = () => {
    setResolve(undefined);
    say("Removed the Resolve project.", true);
  };

  /** A .drp dropped anywhere on the editor becomes the Resolve project (Rust checks and stages it). */
  const addDrp = async (path: string, others: number) => {
    const name = drpName(path.split(/[\\/]/).pop() ?? path);
    setAddingDrp(name);
    try {
      const drp = await editorAddResolveDrp(sourceKey, path);
      change((d) => {
        const { resolve: was, ...rest } = d.draft;
        return {
          ...d,
          draft: { ...rest, resolve: was ? { ...was, drp } : { drp, folder: defaultFolder(d.draft.fields), on: true } },
        };
      });
      say(
        others
          ? `${name} is the Resolve project now. A template has one, so the other .drp files were left out.`
          : `${name} is the Resolve project now. New jobs get their own copy in Resolve.`,
        true,
      );
    } catch (e) {
      say(errorMessage(e));
    } finally {
      setAddingDrp(null);
    }
  };

  const dropTarget = (x: number, y: number): string | null => {
    const ratio = window.devicePixelRatio || 1;
    const el = document.elementFromPoint(x / ratio, y / ratio)?.closest("[data-drop-path]");
    return el ? (el.getAttribute("data-drop-path") ?? null) : null;
  };
  const addFilesRef = useRef(addFiles);
  const addDrpRef = useRef(addDrp);
  const drpOnlyRef = useRef(false);
  useEffect(() => {
    addFilesRef.current = addFiles;
    addDrpRef.current = addDrp;
  });
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((e) => {
        const ev = e.payload;
        if (ev.type === "enter") {
          const drpOnly = ev.paths.length > 0 && ev.paths.every(isDrp);
          drpOnlyRef.current = drpOnly;
          setDraggingDrp(drpOnly);
          if (!drpOnly) setDropPath(dropTarget(ev.position.x, ev.position.y));
        } else if (ev.type === "over") {
          // A .drp goes to the Resolve project wherever it's dropped: no folder lights up.
          if (!drpOnlyRef.current) setDropPath(dropTarget(ev.position.x, ev.position.y));
        } else if (ev.type === "drop") {
          const into = dropTarget(ev.position.x, ev.position.y);
          setDropPath(null);
          setDraggingDrp(false);
          drpOnlyRef.current = false;
          const drps = ev.paths.filter(isDrp);
          const rest = ev.paths.filter((p) => !isDrp(p));
          if (drps.length) void addDrpRef.current(drps[0], drps.length - 1);
          if (rest.length) void addFilesRef.current(into ?? "", rest);
        } else {
          setDropPath(null);
          setDraggingDrp(false);
          drpOnlyRef.current = false;
        }
      })
      .then((u) => (unlisten = u))
      .catch(() => {});
    return () => unlisten?.();
  }, []);

  // ── Right-click ──

  const menu = async (id: RowId) => {
    const isFile = id.startsWith("file:");
    const at = !isFile && id !== ROOT ? locate(nodes, id) : null;
    const node = at?.node;
    const ticks = draft.fields.filter((f) => f.type === "bool");
    const picks = draft.fields.filter((f) => f.type === "multi");
    const path = id === ROOT ? "" : (at?.path ?? "");
    type Item = { text: string; action?: () => void; enabled?: boolean } | { item: "Separator" } | { text: string; items: Item[] };
    let items: Item[];
    if (isFile) {
      items = [
        { text: "Rename", action: () => startRename(id) },
        { text: "Remove", action: () => remove(id) },
      ];
    } else if (node?.each) {
      items = [{ text: "Stop making one folder for each choice", action: () => setEach(parentOf(nodes, id) ?? "", null) }];
    } else {
      const eachKid = node?.kids.find((k) => k.each);
      items = [
        { text: "Add folder inside", action: () => addInside(node ? id : null) },
        { text: "Add files…", action: () => void addFiles(path, null) },
        { text: "Rename", action: () => startRename(id) },
      ];
      if (node) {
        const folderLabel = node.name.replace(/[{}]/g, "");
        items.push(
          { item: "Separator" },
          {
            text: "Only make this if…",
            items: [
              ...ticks.map((f) => ({ text: `“${f.label}” is ticked`, action: () => setWhen(id, f.key) })),
              { text: "A new yes / no question", action: () => setWhen(id, newBlank(`Make ${folderLabel}?`, "bool")) },
              ...(node.when ? [{ item: "Separator" as const }, { text: "Always make it", action: () => setWhen(id, null) }] : []),
            ],
          },
          {
            text: "One folder for each…",
            items: [
              ...picks.map((f) => ({ text: `choice in “${f.label}”`, action: () => setEach(id, f.key) })),
              { text: "A new pick-several question", action: () => setEach(id, newBlank(`${folderLabel} choices`, "multi")) },
              ...(eachKid ? [{ item: "Separator" as const }, { text: "Stop", action: () => setEach(id, null) }] : []),
            ],
          },
          node.kit
            ? { text: "No client files here", action: () => setKit(null) }
            : { text: "Put the client's files here", action: () => setKit(id), enabled: canHoldKit(nodes, id) },
          { item: "Separator" },
          { text: "Delete", action: () => remove(id) },
        );
      }
    }
    try {
      const m = await Menu.new({ items: items as Parameters<typeof Menu.new>[0] extends { items?: infer I } ? I : never });
      await m.popup();
    } catch (e) {
      console.warn("Right-click menu didn't open", e);
    }
  };

  // ── The order New Project asks the questions in (drag a name along the bottom line) ──

  const isClient = (key: string) => draft.fields.find((f) => f.key === key)?.type === "client";
  /** When a drag ended: the click that may follow it isn't a click on the name. */
  const draggedAt = useRef(0);
  const askDrag = usePointerDrag({
    attr: "data-ask-key",
    axis: "x",
    ignore: "input, select, textarea",
    canDrop: (key, target) => key !== target && !isClient(key),
    canPlace: (target, where) => !(where === "before" && isClient(target)),
    onDrop: (key, t) => {
      draggedAt.current = Date.now();
      const where = t.where === "after" ? "after" : "before";
      change((d) => {
        const fields = moveField(d.draft.fields, key, t.id, where);
        return fields === d.draft.fields ? d : { ...d, draft: { ...d.draft, fields } };
      });
    },
  });

  // ── Keyboard ──

  // Edit → Undo / Redo in the macOS menu bar, when the focus isn't in a text box.
  useEffect(() => {
    const onStep = (e: Event) => {
      const step = (e as CustomEvent<EditStep>).detail;
      setRenaming(null);
      setHist((h) => (step === "redo" ? redo(h) : undo(h)));
    };
    window.addEventListener(EDIT_STEP_EVENT, onStep);
    return () => window.removeEventListener(EDIT_STEP_EVENT, onStep);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = isMac() ? e.metaKey : e.ctrlKey;
      // On macOS the menu bar's Undo / Redo own Cmd+Z and Shift+Cmd+Z (see above).
      const menuOwned = isMac() && (e.key === "z" || e.key === "Z");
      if (mod && !menuOwned && !isTyping(e.target) && (e.key === "z" || e.key === "Z" || e.key === "y")) {
        e.preventDefault();
        setRenaming(null);
        if (e.key === "y" || e.shiftKey) setHist((h) => redo(h));
        else setHist((h) => undo(h));
        return;
      }
      if (e.key === "Escape" && !isTyping(e.target)) {
        if (blank) setBlank(null);
        else if (dirty.current && !confirmClose) setConfirmClose(true);
        else onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const onTreeKey = (e: React.KeyboardEvent) => {
    if (isTyping(e.target) || !selected) return;
    // The Resolve project's buttons sit under the tree; the tree's keys aren't for them.
    if ((e.target as HTMLElement).closest?.("[data-resolve-section]")) return;
    const order = rowOrder(nodes, files);
    const i = order.indexOf(selected);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = order[Math.min(order.length - 1, Math.max(0, i + (e.key === "ArrowDown" ? 1 : -1)))];
      if (next) setSelected(next);
    } else if (e.key === "Enter" || e.key === "F2") {
      e.preventDefault();
      startRename(selected);
    } else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      remove(selected);
    } else if (e.key === "Tab" && selected !== ROOT && !selected.startsWith("file:")) {
      e.preventDefault();
      shift(selected, e.shiftKey);
    }
  };

  // ── Save / close ──

  const save = () => {
    setSaving(true);
    setError(null);
    saveTemplate(sourceKey, full, mode)
      .then(onSaved)
      .catch((e) => {
        setError(errorMessage(e));
        setSaving(false);
      });
  };
  const close = () => {
    if (dirty.current && !confirmClose) setConfirmClose(true);
    else onClose();
  };

  const asks = draft.fields;
  // Only worth dragging with two questions that can move (the client one stays first).
  const canReorder = asks.filter((f) => f.type !== "client").length > 1;

  return (
    <div className={styles.scrim}>
      <div
        ref={sheetRef}
        tabIndex={-1}
        className={styles.sheet}
        role="dialog"
        aria-modal="true"
        aria-label={mode === "customize" ? `Customize “${start.name}”` : "Template"}>
        <header className={styles.head}>
          <input
            className={styles.nameInput}
            value={draft.name}
            maxLength={60}
            aria-label="Template name"
            onChange={(e) => {
              const name = e.target.value;
              change((d) => ({ ...d, draft: { ...d.draft, name } }));
            }}
          />
          <Button onClick={() => setShowMore((v) => !v)} aria-expanded={showMore}>
            {showMore ? "Less" : "More…"}
          </Button>
        </header>
        {showMore && (
          <div className={styles.more}>
            <label htmlFor="tpl-desc">Description</label>
            <input
              id="tpl-desc"
              className={styles.input}
              value={draft.description}
              maxLength={120}
              placeholder="Shown under the name in New Project"
              onChange={(e) => {
                const description = e.target.value;
                change((d) => ({ ...d, draft: { ...d.draft, description } }));
              }}
            />
            <label htmlFor="tpl-title">Title in Nest's list</label>
            <input
              id="tpl-title"
              className={`${styles.input} ${styles.mono}`}
              value={draft.title ?? ""}
              placeholder="Automatic (the answers, joined)"
              onChange={(e) => {
                const title = e.target.value;
                change((d) => ({ ...d, draft: { ...d.draft, title: title === "" ? undefined : title } }));
              }}
            />
          </div>
        )}

        <p className={styles.lead}>A new project gets these folders. Click a name to change it.</p>

        <div ref={treeRef} className={styles.tree} tabIndex={0} onKeyDown={onTreeKey}>
          <Tree
            folderName={draft.folderName}
            examples={examples}
            nodes={nodes}
            files={files}
            fields={draft.fields}
            selected={selected}
            renaming={renaming}
            fresh={newRow}
            dropPath={dropPath}
            onSelect={setSelected}
            onStartRename={startRename}
            onRename={rename}
            onCancelRename={() => setRenaming(null)}
            onAddInside={addInside}
            onAddFile={(path) => void addFileMenu(path)}
            onMenu={(id) => void menu(id)}
            onMove={move}
            onChip={(row, key, index) => setBlank({ key, row, index })}
            onNewBlank={(label) => newBlank(label)}
          />
          <ResolveSection
            sessionId={sourceKey}
            value={draft.resolve}
            fields={draft.fields}
            folderName={draft.folderName}
            onChange={setResolve}
            onRemove={removeResolve}
            onNewBlank={(label) => newBlank(label)}
            onChip={(key) => setBlank({ key })}
            adding={addingDrp}
            dropping={draggingDrp}
          />
          {toast && (
            <div className={styles.note} role="status">
              {toast.text}
              {toast.undo && (
                <Button
                  onClick={() => {
                    setHist((h) => undo(h));
                    setToast(null);
                  }}
                >
                  Undo
                </Button>
              )}
            </div>
          )}
        </div>

        {blank && (
          <QuestionSettings
            questionKey={blank.key}
            fields={draft.fields}
            written={chipTransform}
            onChange={updateField}
            onRemove={removeBlank}
            onClose={() => setBlank(null)}
          />
        )}

        <div
          className={`${styles.asks} ${askDrag.dragging ? styles.asksDragging : ""}`}
          aria-label="What New Project asks"
        >
          <span className={styles.asksLabel}>New Project will ask for:</span>
          {asks.length === 0 && <span>nothing yet</span>}
          {asks.map((f, i) => {
            const movable = f.type !== "client" && canReorder;
            const drop = askDrag.target?.id === f.key ? askDrag.target.where : null;
            return (
              <span
                key={f.key}
                data-ask-key={f.key}
                className={[
                  styles.ask,
                  movable ? styles.askMovable : "",
                  askDrag.dragging === f.key ? styles.dragging : "",
                  drop === "before" ? styles.askDropBefore : drop === "after" ? styles.askDropAfter : "",
                ].join(" ")}
                {...(movable ? askDrag.handlers(f.key) : {})}
              >
                {movable && <span className={styles.grip} aria-hidden="true" />}
                <button
                  type="button"
                  className={`${styles.askLink} ${blank?.key === f.key ? styles.askOpen : ""}`}
                  aria-expanded={blank?.key === f.key}
                  title={movable ? "Click to change. Drag to ask it earlier or later." : undefined}
                  onClick={() => {
                    if (Date.now() - draggedAt.current < 400) return;
                    setBlank(blank?.key === f.key ? null : { key: f.key });
                  }}
                >
                  {blankLabel(f.key, draft.fields)}
                </button>
                {i < asks.length - 1 && ","}
              </span>
            );
          })}
          <span className={styles.spacer} />
          <Button
            onClick={() => {
              const key = newBlank("New question");
              setBlank({ key });
            }}
          >
            Add a question
          </Button>
        </div>

        <footer className={styles.foot}>
          <div className={styles.footMsg} role={problems.length || error ? "alert" : undefined}>
            {confirmClose ? (
              <span>Close without saving? Your changes will be lost.</span>
            ) : error ? (
              <span className={styles.problem}>{error}</span>
            ) : problems.length ? (
              <span className={styles.problem}>
                {problems.length === 1 ? problems[0] : `${problems[0]} (and ${problems.length - 1} more)`}
              </span>
            ) : (
              <span className={styles.hint}>
                {note ?? "Changes apply to new projects only. Projects you already made are never touched."}
              </span>
            )}
          </div>
          {confirmClose ? (
            <>
              <Button onClick={() => setConfirmClose(false)}>Keep editing</Button>
              <Button onClick={onClose}>Close without saving</Button>
            </>
          ) : (
            <>
              <Button onClick={close}>Cancel</Button>
              <Button variant="primary" disabled={saving || !fresh || problems.length > 0} onClick={save}>
                {saving ? "Saving…" : "Save template"}
              </Button>
            </>
          )}
        </footer>
      </div>
    </div>
  );
}

/** Row id of a file entry (for tests). */
export { fileRow };
