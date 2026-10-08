# Nest v1: Build Spec

As of 2026-09-28 · Owner: Bernard · Companion to the PRD (private notes, v0.3)

The PRD says what and why. This says exactly how v1 behaves, so Claude Code builds it without guessing. If this file and the PRD disagree, this file wins for v1.

**Targets:** Windows 10/11 (x64) and macOS 13+ (Apple Silicon + Intel via universal build). Every feature ships on both or it doesn't ship.

---

## 1. v1 scope

In:

- First-run setup
- Templates (JSON files, import/export; in-app template editor in v0.2, see §12a #19)
- New Project sheet with dynamic fields and live folder preview
- Create project: folders, starter files, `.project.json`
- Project list, search, filters, inspector
- Settings: jobs roots, job code pattern, templates, appearance
- Rescan / rebuild index
- Packaging for both OSes + auto-update

Out (later milestones): drop-to-rename, export naming, Resolve/Premiere/AE projects, ops app connection, delivery check, licensing.

---

## 2. Architecture rules

1. **All filesystem writes happen in Rust**, never in the webview. The UI calls Tauri commands. No `tauri-plugin-fs` write permissions exposed to JS.
2. **Plan, then apply.** Creating a project is two steps:
   - `plan_project(template, values) → Plan` is a pure function: it returns the exact list of folders and files to create and touches nothing.
   - `apply_plan(plan) → Result` executes it.
   - The New Project preview renders the Plan, so **the preview is always exactly what gets created.**
3. **Never delete or overwrite user files.** The only deletion allowed in v1 is rollback of items created in the same failed `apply_plan` run, tracked in a list. *Exception (2026-10-02): your own templates may go to the Recycle Bin / Trash, always restorable (§12a #16). Never projects.*
4. **The folder is the truth.** `.project.json` is canonical. SQLite is a cache that can be rebuilt from a rescan at any time.
5. **Atomic writes** for every JSON file (write `*.tmp`, fsync, rename).
6. **Paths are platform-native** in Rust (`PathBuf`). The UI only displays them.

### Stack

| Piece | Choice |
|---|---|
| Shell | Tauri 2 |
| UI | React 18 + TypeScript + Vite |
| Styling | CSS modules or vanilla CSS with tokens (section 8). No Tailwind, no component library, no web-looking widgets |
| State | Zustand (small) or React context. No Redux |
| Local index | SQLite via `rusqlite` (in Rust, not JS) |
| Settings | JSON file in app config dir, read/written by Rust |
| Window material | `window-vibrancy` crate (macOS vibrancy, Windows Mica) |
| Reveal/open | `tauri-plugin-opener` (reveal in Finder/Explorer) |
| Dialogs | `tauri-plugin-dialog` (native folder pickers) |
| Single instance | `tauri-plugin-single-instance` |
| Window state | `tauri-plugin-window-state` |
| Updates | `tauri-plugin-updater` |

### Where things live

| Thing | macOS | Windows |
|---|---|---|
| Settings + templates | `~/Library/Application Support/com.intrvl.nest/` | `%APPDATA%\com.intrvl.nest\` |
| Index DB | same dir, `index.sqlite` | same dir, `index.sqlite` |
| Logs | `~/Library/Logs/com.intrvl.nest/` | `%APPDATA%\com.intrvl.nest\logs\` |

Use Tauri's path resolver, never hardcode.

---

## 3. Screens

### 3.1 Main window

Three panes. Minimum size 900×560. Remembers size/position.

```
┌──────────────┬───────────────────────────────┬────────────────┐
│ Sidebar      │ Project list                  │ Inspector      │
│ (translucent)│                               │                │
│              │ [Search ⌘K        ] [+ New]   │ VX-J07         │
│ SPACES       │                               │ KIRA Summer Nights│
│  Work     12 │ VX-J07  KIRA Summer Nights   ●   │                │
│  Personal  3 │ VX-J06  NMIXX Luther      ●   │ Client  VX     │
│              │ BF-L02  Look dev test     ○   │ Type    MV     │
│ STATUS       │                               │ Created 28 Sep │
│  Active      │                               │ Template ...   │
│  Done        │                               │                │
│              │                               │ [Open folder]  │
│              │                               │ [Reveal]       │
│ ⚙ Settings   │  3 projects                   │ Status ▾       │
└──────────────┴───────────────────────────────┴────────────────┘
```

- **Sidebar:** Spaces with counts, Status filter (Active / Done / All), Settings at the bottom. macOS: translucent (vibrancy `Sidebar`). Windows: Mica on the window.
- **List:** rows show job code (monospace), title, status dot. Sort: newest first by default; click header to sort by code/title/date. Double-click = open folder. Enter = open folder. Space = toggle inspector.
- **Inspector:** read-only fields from manifest, status dropdown (Active / Done), Open folder, Reveal, "Copy path". Collapsible.
- **Empty states:** no projects ("Create your first project" + button), no search results, jobs root missing (section 6).

### 3.2 New Project sheet

Modal sheet (macOS sheet style: slides from the window's top; Windows: centred dialog). Width 520.

Order top to bottom:

1. **Template** dropdown (remembers last used).
2. **Fields** rendered from the template's `fields` (section 4). Required fields marked.
3. **Space** dropdown.
4. **Job code** (read-only preview, e.g. `VX-J07`, auto-generated). Small "edit" link lets you override before creating.
5. **Folder name** preview (e.g. `VX-J07_KIRA_SummerNights`).
6. **Preview tree**: collapsible, shows every folder and starter file from the Plan. Updates live as fields change (debounce 150ms).
7. Buttons: Cancel (Esc), Create (Cmd/Ctrl+Enter). Create is disabled until required fields are valid.

After Create: sheet closes, new project selected in list, a quiet toast "Created VX-J07" with "Open folder".

### 3.3 Settings (separate window, native preferences style)

Tabs:

- **General:** jobs root(s) for this machine (add/remove, native picker), default space, launch at login (off by default).
- **Job codes:** in plain words (§12a #18): example code, styles, letter for this computer, code for personal jobs, pattern under Advanced.
- **Templates:** list of installed templates (name, version, source file), Import (.nesttemplate or .json), Export, Reveal templates folder, Duplicate. No visual editor in v1.
- **Spaces:** add/rename/reorder/delete (delete only if empty).
- **Appearance:** System / Light / Dark.
- **About:** version, check for updates, open logs folder.

### 3.4 First run

Shown when no settings file exists.

1. Welcome: one line on what Nest does. Continue.
2. **Choose jobs folder** (native picker). Offer to create `Nest Jobs` in Documents if they skip.
3. **Pick starter templates** (checkboxes, all bundled ones pre-ticked).
4. **Default spaces:** Work, Personal (editable).
5. **Letter for this computer** (job codes; suggested W on Windows, M on a Mac).
6. Done → main window with empty state.

If the chosen jobs folder already contains folders with `.project.json`, run a scan and show "Found N projects".

---

## 4. Template format

A template is a JSON file (`*.nest.json`) plus an optional folder of starter files. Bundled as `.nesttemplate` (a zip) for import/export.

```json
{
  "schema": 1,
  "id": "grading-mv",
  "version": 1,
  "name": "Grading: MV",
  "description": "Music video grade with delivery ratios",
  "fields": [
    { "key": "client", "label": "Client", "type": "client", "required": true },
    { "key": "artist", "label": "Artist", "type": "text", "required": true, "max": 60 },
    { "key": "song", "label": "Song", "type": "text", "required": true, "max": 60 },
    { "key": "ratios", "label": "Delivery ratios", "type": "multi",
      "options": ["16x9", "9x16", "1x1", "4x5"], "default": ["16x9"] },
    { "key": "lookdev", "label": "Include look dev", "type": "bool", "default": false }
  ],
  "folderName": "{jobCode}_{artist}_{song}",
  "tree": [
    "01_REF",
    "02_FOOTAGE",
    "03_PROJECT",
    "04_LUTS",
    "05_STILLS",
    { "name": "05_STILLS/LOOKDEV", "when": "lookdev" },
    "06_EXPORTS",
    { "name": "07_DELIVERY/{item}", "each": "ratios" }
  ],
  "files": [
    { "from": "starter/NOTES.md", "to": "NOTES.md", "fill": true },
    { "from": "starter/luts/", "to": "04_LUTS/" }
  ]
}
```

### Rules

- **Field types v1:** `text`, `longtext`, `bool`, `select` (one of `options`), `multi` (many of `options`), `date`, `client` (text + 2 to 5 char code, see section 5).
- **`client` = the billing party (who pays), shown as "Billed to"** whatever the template's label says. The artist or brand the work is for is a separate required `text` field with key `artist`, labelled "Artist / Brand", in every bundled grading/edit template (MV, PV, Look dev, Edit General; not Blank). Artists are never clients. (2026-10-01)
- **Variables:** `{fieldKey}` plus built-ins `{jobCode}`, `{date}` (YYYY-MM-DD), `{yymm}`, `{year}`, `{space}`, `{templateName}`.
- **Transforms:** `{artist|pascal}` (`SummerNights`), `{artist|upper}`, `{artist|lower}`, `{artist|kebab}`. Default in folder names: pascal with spaces removed.
- **`when`:** a bool field key, or `!key` for not. Nothing fancier in v1.
- **`each`:** a `multi` field key; `{item}` is the current value. Empty selection = folder not created.
- **Nesting:** use `/` in `name`. Parents are created automatically. Always `/`, Rust converts per OS.
- **`files`:** `from` is relative to the template's folder. `fill: true` replaces variables inside text files (UTF-8 only; binary files are copied as-is). A trailing `/` copies a whole folder.
- **Validation on import:** unknown field types, a `when`/`each` pointing at a missing or wrong-type field, `..` in any path, absolute paths, or duplicate field keys → refuse the import with a clear message.
- **Versioning:** a project's manifest records `template.id` + `template.version`. Changing a template never touches existing projects.

### Bundled templates (v1, revised 2026-10-02: generic, see §12a #17)

1. Blank (just the project folder + NOTES.md)
2. General project (01_BRIEF, 02_REFERENCE, 03_ASSETS, 04_WORKING, 05_EXPORTS, 06_ADMIN)
3. Video (01_BRIEF, 02_FOOTAGE, 03_AUDIO, 04_PROJECT, 05_EXPORTS/<format>, 06_GRAPHICS if ticked)
4. Design (01_BRIEF, 02_REFERENCE, 03_ASSETS/FONTS + IMAGES, 04_WORKING, 05_EXPORTS/DIGITAL, PRINT if ticked)
5. Photo shoot (01_BRIEF, 02_RAW, 03_SELECTS, 04_EDITS, 05_DELIVERY)

Each asks Billed to, Project name and Start date. Only these ids load from the bundle (`BUILT_IN_IDS`). Bernard's colorist templates (Grading: MV, Grading: PV, Edit: General, Look dev) are Bernard's own, kept as files in `private/extras/bernard-templates/` (private notes).

---

## 5. Job codes and names

- **Pattern:** default `{clientCode}-J{seq:02}`. Local-only projects (always true in v1, since the ops app connection comes later) use the marker `L`: `VX-L01`.
- **Client code:** 2 to 5 chars, A–Z and 0–9, uppercased. The `client` field suggests previously used clients (from the index) with their codes. It is the **billing client's** code (`VX-L03` for a NOVA BAND job paid by Vertex Studio), never an artist code. The job code is kept in the manifest (`jobCode`), not in the folder name (§12a #1). (2026-10-01)
- **Sequence:** per client code per marker. Next = highest existing `seq` for that code found in the index **and** on disk + 1. No separate counter file (so two machines on a shared drive agree).
- **Collision check at Create:** if the job code or folder name already exists on disk, bump `seq` and re-plan. Show the new code in the toast.
- **Sanitiser** (applied to every generated name):
  - strip `< > : " / \ | ? *` and control chars
  - trim trailing dots and spaces
  - reject Windows reserved names (CON, PRN, AUX, NUL, COM1–9, LPT1–9), case-insensitive
  - collapse whitespace; max 80 chars per segment
  - NFC-normalise Unicode (Korean/Chinese names must survive on both OSes)

---

## 6. Edge cases (must be handled in v1)

| Case | Behaviour |
|---|---|
| Jobs root missing (drive unplugged, NAS offline) | List shows projects from index greyed out with "Offline". Banner: "Jobs folder not found: D:\Jobs". Create disabled for that root. No errors spammed |
| Target folder already exists | Bump seq (section 5). Never merge into an existing folder |
| Permission denied mid-create | Roll back items created in this run, show which path failed and why |
| Path too long on Windows (>260) | Plan step flags it before Create, suggests shortening fields. Enable long path support in the manifest but don't rely on it |
| `.project.json` corrupted / hand-edited badly | Project shows with a warning icon; inspector says "Manifest unreadable" + Reveal. Never auto-fix or overwrite |
| `.project.json` with unknown fields | Preserve them on every write |
| Same project seen on two roots | Show once, note both paths in inspector |
| Folder renamed or moved outside Nest | Next rescan picks it up by `id` in the manifest; path updated in index |
| Folder deleted outside Nest | Removed from index on rescan. No prompt |
| Two Nest instances | Single-instance plugin focuses the existing window |
| Case-only collisions (`vx-j07` vs `VX-J07`) | Treated as collisions on both OSes (Windows and default macOS are case-insensitive) |
| Template starter file missing | Plan shows it with a warning; Create still allowed without it |
| Cloud folders (iCloud, Dropbox, OneDrive) | Allowed. Don't read files we didn't write (avoids forcing downloads) |

---

## 7. Rescan

- Runs on launch (background, non-blocking), on window focus if last scan > 10 min, and on demand (Cmd/Ctrl+R).
- Walks each jobs root **2 levels deep** looking for `.project.json`. Does not descend into folders that have one.
- Skips hidden/system folders, `node_modules`, `.git`, cache folders.
- Target: 500 projects scanned in under 2 seconds on SSD.

---

## 8. Design tokens

Premium means quiet, consistent, fast. Follow the OS, don't imitate a website.

### Type

| Token | macOS | Windows |
|---|---|---|
| `--font-ui` | `-apple-system, "SF Pro Text"` | `"Segoe UI Variable Text", "Segoe UI"` |
| `--font-mono` (job codes) | `"SF Mono", ui-monospace` | `"Cascadia Mono", Consolas` |
| Body | 13px / 18px | 14px / 20px |
| Small / secondary | 11px | 12px |
| Title (inspector) | 15px semibold | 16px semibold |

### Spacing and shape

- Base unit 4px. Common: 4, 8, 12, 16, 24.
- Row height: 28px (mac), 32px (win).
- Radius: 6px controls, 10px sheets/cards.
- Borders: 1px hairlines, low contrast.

### Colour

- Use system accent colour for selection and primary buttons (read via Tauri/OS; fallback `#0A84FF`).
- Light: background transparent over window material, text `rgba(0,0,0,0.85)`, secondary `0.5`, separators `0.1`.
- Dark: text `rgba(255,255,255,0.88)`, secondary `0.55`, separators `0.12`.
- Status: Active = accent dot, Done = hollow grey dot. No other colours in v1.

### Behaviour rules

- `user-select: none` on all chrome; text selectable only in inspector values.
- No bounce/overscroll. No hover underlines. Cursor stays default on buttons (native apps don't use pointer hands).
- Focus rings only on keyboard focus (`:focus-visible`), using accent colour.
- Motion: 150–200ms ease-out for sheets and panes; respect reduced motion.
- Native context menus (Tauri menu API) on list rows: Open, Reveal, Copy path, Copy job code, Mark done.
- Native menu bar (mac) / window menu (win): File (New Project, Rescan), Edit, View (Toggle sidebar, Toggle inspector), Window, Help.

### Shortcuts

| Action | macOS | Windows |
|---|---|---|
| New project | Cmd+N | Ctrl+N |
| Search | Cmd+K / Cmd+F | Ctrl+K / Ctrl+F |
| Create (in sheet) | Cmd+Enter | Ctrl+Enter |
| Rescan | Cmd+R | Ctrl+R / F5 |
| Settings | Cmd+, | Ctrl+, |
| Toggle inspector | Cmd+Opt+I | Ctrl+Alt+I |
| Open folder | Enter | Enter |

---

## 9. Milestones and acceptance criteria

Each milestone is done only when every box is ticked **on both Windows and macOS**.

### M0: Scaffold

- [ ] Tauri 2 app runs on both OSes with a blank three-pane window
- [ ] Vibrancy (mac) and Mica (win) applied; falls back to solid on unsupported systems
- [ ] Single instance + window state working
- [ ] `CLAUDE.md` in repo, gate commands pass (section 10)

### M1: Templates + plan

- [ ] Bundled templates load; invalid template import is refused with a clear message
- [ ] `plan_project` unit-tested: variables, transforms, `when`, `each`, sanitiser, reserved names, long paths
- [ ] Job code generation unit-tested incl. collisions and gaps (e.g. J01, J03 exist → next is J04)

### M2: Create

- [ ] New Project sheet renders fields from the template, validates required fields
- [ ] Preview tree matches exactly what Create produces (integration test compares Plan to disk)
- [ ] Create writes folders, starter files (with fill), and `.project.json` (hidden on Windows via file attribute)
- [ ] Failure mid-create rolls back only what this run created
- [ ] Korean/Chinese characters in fields work on both OSes

### M3: Browse

- [ ] List, search (code, title, client, any field value), Space + Status filters
- [ ] Inspector with Open folder, Reveal, Copy path, status change (writes manifest atomically)
- [ ] Rescan on launch/focus/manual; offline roots handled per section 6
- [ ] Context menus and all shortcuts in section 8

### M4: Settings + first run

- [ ] First run flow completes and lands on empty state
- [ ] Jobs roots add/remove; Spaces CRUD; job code pattern with live preview
- [ ] Template import/export as `.nesttemplate`
- [ ] Appearance follows system and manual override

### M5: Packaging + updates

- [ ] Windows: NSIS installer `.exe` builds; installs per-user without admin
- [ ] macOS: universal `.dmg` builds; app runs on Apple Silicon and Intel
- [ ] Auto-update: app detects a newer version, downloads, verifies signature, restarts
- [ ] Version shown in About; logs folder opens from About

**v1 done** = M0 to M5 ticked, and Bernard has used it for real jobs for 2 weeks without hand-fixing folders.

---

## 10. Testing

- **Rust unit tests** (`cargo test`): template engine, sanitiser, job codes, manifest read/write/preserve-unknown.
- **Rust integration tests:** create projects into a temp dir, assert the tree on disk equals the Plan; simulate permission errors for rollback.
- **Frontend tests** (Vitest): field rendering and validation, search filtering.
- **Never test against real jobs folders.** Tests use temp dirs only. Dev builds default to a `NestDev` jobs root.
- **Gate before every commit:** `cargo fmt --check && cargo clippy -- -D warnings && cargo test && npm run typecheck && npm run lint && npm test`.
- **Manual pass per milestone on both OSes** using the checklist in section 9.

---

## 11. Packaging and updates

### Outputs

| OS | File | Notes |
|---|---|---|
| Windows | `Nest_x.y.z_x64-setup.exe` (NSIS) | Per-user install, no admin. `.msi` optional later for studios |
| macOS | `Nest_x.y.z_universal.dmg` | One build for Apple Silicon + Intel |

`tauri build` produces these. **Each OS must build its own installer**: the `.exe` on Windows, the `.dmg` on a Mac. Two options:

1. **Local:** build on your PC for Windows, on the MacBook for macOS. Fine for personal use.
2. **CI (recommended once selling):** GitHub Actions with `tauri-action` builds both on GitHub's Windows and macOS runners from one tag push, and uploads them to a GitHub Release. macOS runner minutes cost more on private repos [check current GitHub pricing].

### Updates

- `tauri-plugin-updater` checks a small JSON manifest (`latest.json`) on launch and every 24h.
- Updates are signed with an **updater key pair** (generated once with `tauri signer generate`). The public key is in the app; the private key stays secret (password manager + CI secret). Lose it and existing installs can't update.
- Hosting: **GitHub Releases** for the binaries and `latest.json` (`tauri-action` generates both). Simple, free, built for large files.
- **Vercel is for the website, not the binaries.** Use it for the Nest landing page, docs, and (later) licence endpoints. Installers are large files and Vercel isn't built to serve downloads. Also Vercel's Hobby plan is for non-commercial use, so a paid Nest site would need Pro [check current Vercel terms].

### Signing (not needed for personal use, needed before selling)

| OS | Without signing | With signing |
|---|---|---|
| Windows | SmartScreen shows "Windows protected your PC". Click More info → Run anyway | Needs a code signing certificate (OV/EV, or Azure Trusted Signing). Warning fades as reputation builds |
| macOS | Gatekeeper blocks the app when downloaded from the internet. Open via System Settings → Privacy & Security → Open Anyway. Builds made on your own Mac aren't blocked | Apple Developer Program ($99/yr), Developer ID signing + notarization (Tauri supports both in the build config) |

For v1 personal use: build locally, skip signing, turn on the updater only when CI exists.

---

## 12a. Clarifications (2026-09-28)

These fill gaps in the sections above. Where they differ, the clarification wins.

1. **Folder naming is per template.** Bernard's own template (v1.6) uses `{start|yymmdd}_{name|caps}` → `250608_CLIENTX_BRAND_FILM`: a start-date field (default today) plus the name in CAPS_UNDERSCORES, with **no job code in the folder name**. The code lives in the manifest and in Nest. **All bundled templates follow the same folder-name convention:** a Start date field (default today) and a `{start|yymmdd}_{name|caps}` folder name. Folders inside are numbered per template; the v1.6 numbering (00_REFERENCE, 01_FOOTAGES, 05_FINAL/<ratio>, 06_DI…) stays in Bernard's own templates. (Revised 2026-10-02.)

Bernard's own template is named "Project". The year is never part of the job code. (Revised 2026-09-28: was a fixed `{yymm}_{jobCode}_…` prefix.)
2. **Job code pattern:** default `{clientCode}-{marker}{seq:02}`. The marker is **one letter per computer** (§12a #18); `J` = ops-issued (later) and can't be chosen. Settings → Job codes edits the pattern and the letter.
3. **Manifest shape:** template field values go in `fields: { key: value }`. Top level keeps `schema`, `id`, `jobCode`, `title`, `client {name, code}`, `space`, `status` (`active` | `done`), `template {id, version}`, `createdAt`, `links`. Unknown keys are preserved (§6).
4. **Title:** templates may set `title` (a pattern, e.g. `"{artist} {song}"`). If they don't, it's the non-empty `text` field values joined with spaces, in field order.
5. **Sanitiser failures block Create** (reserved name, empty after cleaning). The Plan shows a plain error; nothing is silently renamed.
6. **Pascal-by-default applies to field values and `{item}` in names only.** Built-ins (`{jobCode}`, `{date}`…) are never transformed; values filled inside starter files are used as typed. `/` and `\` in values never create subfolders.
7. **Every template has exactly one `client` field.** `{client}` = client name, `{clientCode}` = built-in for its code.
8. **Windows path limits are checked on every OS:** folders ≤ 247 chars, files ≤ 259 (UTF-16 units, including the jobs root). Only the worst offender is reported.
9. **Template validation also refuses** unknown variables, unknown transforms, `{item}` outside `each`, unknown JSON keys, and defaults that don't fit their field.
10. **Folders differing only in case are merged**; a file and a folder on the same path is an error. A missing required field shows its label in the preview (Create stays blocked; with No client it's optional and stays empty, §12a #22).
11. **Unknown `{variables}` inside filled starter files are left as-is** (files may legitimately contain braces).
12. **Duplicate job codes across machines are detected, not prevented.** Two machines creating the same client's job at the same moment on a shared drive can both get the same code when their folder names differ. A rescan flags duplicate codes with a warning, and Bernard fixes them by hand. There's no lock or claim file: sync services delay files, so a lock wouldn't be reliable anyway. (2026-09-28)
13. **More template vocabulary:**
    - transforms `caps` (`VERTEX_STUDIO_FILM`) and `yymmdd` (ISO date → `250608`);
    - a built-in `{yymmdd}`;
    - a date field default of `"today"`.

    If a folder name has no `{jobCode}` and a folder with that name already exists, that's an error ("change the name or the date"): bumping the code can't free the name.
14. **User templates** live in the app config folder's `templates/`, next to the bundled ones, and win on the same id. Bernard's v1.6 template is installed there, not in the repo. It deliberately skips Premiere `.prproj` files and Premiere's auto-save folders until v2.

16. **Templates in Settings (M4):**
    - **Import:** `.nesttemplate` (zip) or `.json`; unsafe or invalid packages are refused, and nothing existing is overwritten.
    - **Decision T1 → replaced by T-A (2026-10-02, Bernard):** one copy per template. Saving, or adding a newer version from another computer, replaces yours, and the previous copy goes to the Recycle Bin / Trash (restorable; the confirmation says so). Version numbers stay internal (they decide which copy is newer) and aren't shown. On first start of v0.3.0, extra old copies are moved to the bin once, with a note. Superseded: ~~a newer version installs alongside it, and only the newest is offered; older ones stay on disk until deleted.~~
    - **Duplicate** always makes a new id.
    - **Delete** works on *your* templates only: after a native confirm, the template is **moved to the Recycle Bin / Trash** (restorable). This is the one exception to "Nest never deletes files", approved by Bernard 2026-10-01, and it never applies to projects. **Extended 2026-10-02 (approved by Bernard):** delete moves *all* versions of that template together, and "Go back to Nest's original" moves your customized version of a built-in to the bin. The confirm says how many versions move; oldest first; anything left behind is reported.
    - **Export:** the Save dialog's own "Replace?" is the only time an existing file is overwritten, by your explicit choice.
    - **Spaces** can be renamed or deleted only while no project uses them. (2026-10-01)
15. **Client = billing party; artist is its own field.** `client` is always who pays (shown "Billed to"); `fields.artist` ("Artist / Brand") is who the work is for. Job codes use the billing client's code. Folder names include the artist (e.g. `260929_NOVA_BAND_BRAND_FILM`); the job code stays in the manifest only. Existing projects that used an artist code as their client code (e.g. `BND-L01`, billed to Acme Music / BND) are left as they are: Nest never rewrites user files. (2026-10-01) Since 2026-10-02 the `artist` field is only in Bernard's own templates; built-ins use `name` ("Project name").
17. **Built-in templates are generic** (Blank, General project, Video, Design, Photo shoot) so Nest suits any creative; colorist templates are Bernard's own. Only ids in `BUILT_IN_IDS`, each in its own folder, load from the bundle, because a Windows update can leave old template folders behind. (2026-10-02)
18. **One job-code letter per computer.** Each computer uses its own letter (first run suggests W on Windows, M on a Mac), so two computers never make the same code. Exactly one letter A–Z; J is refused. The saved default stays `L`, so older settings files keep their letter. Settings → Job codes is in plain words: an explained example code, ready-made styles that all keep the letter, the raw pattern under Advanced. (2026-10-02)
19. **Template editor (v0.2, approved by Bernard 2026-10-02):** edit name, questions and folders in the app (plan: `private/plans/v0.2-settings-redesign.md` Parts C–D). Moved out of §12.
20. **Devices (v0.3.0; approved by Bernard 2026-10-01 as M4b, built 2026-10-03):** see which computer has which project. It's optional, off until turned on in Settings → Devices.
    - **One file per computer:** each computer writes only its own list, `<synced folder>/Nest/devices/<device id>.json` (dev builds: `devices-dev/`).
    - **What's in it:** job code, name, billed to, artist, template, space, status, created date, and the folder path as text.
    - **Other computers' projects** show greyed and read-only ("On MacBook"). New job codes also skip their codes.
    - **Locked decision #9 holds:** Nest only reads and writes files in a folder. Google Drive (or Dropbox, OneDrive, iCloud, a NAS) does the syncing.
    - **Safety:** Nest never creates a missing synced folder and never uses another computer's paths. Every file is capped: 2 MB, 5,000 projects, 50 computers.
    - **"Mark done" from another computer (v0.3.2):** a request goes into the sender's list; the owning computer applies it the first time it sees it, through the same safe status change as a click there, and records it at once. Request numbers only go up (time-based floor), so nothing is applied twice, even after a reinstall. A missing or still-syncing list never cancels anything. Problems that won't fix themselves come back as plain-words refusals; the rest are retried.
21. **Archive (M5; approved by Bernard 2026-10-04; plan: `private/plans/M5-archive.md`).** Moved out of §12. Off by default. In Settings you pick an archive folder (any drive) and "suggest archiving after N days done" (Nest proposes 90). A project is archived because its folder is under the archive folder (locked #5: the folder is the truth); archived projects are read-only in Nest, hidden behind an Archived filter, and can be unarchived. "Archive" on a Done project: same drive = rename; another drive = Nest copies and verifies, then reveals the original and says it's safe to trash. **Nest never deletes (locked #4 holds)** and never archives on its own: it shows "N projects ready to archive" and you click. Folder sizes per subfolder and by file type (video / audio / images / other) are measured in the background and shown in the details panel, so you can see what to clear out in Finder/Explorer before archiving. No "strip caches". No archiving a project that lives on the other computer.
    - **Archive… / Unarchive… button (v0.4.2, 2026-10-08): same drive only.** One rename, after a native confirm; nothing copied or deleted. Archive needs a Done project here, in one place. Unarchive goes back to the jobs folder in `archivedFrom` only if that is one of this computer's jobs folders now, else the first jobs folder. The manifest notes `archivedAt` / `archivedFrom` (a note only; archived is decided by location). Refused with plain words: another drive (drag it yourself; Nest picks it up), a folder of that name already there, a file open (close Resolve), cloud-synced folders (moving can remove them from other computers), names Nest wouldn't make. **Not built:** copying to another drive. The Recycle Bin can permanently delete big or network folders without asking, so Nest won't remove an original.
22. **No client per project + simpler template editor (v0.4.1; Bernard 2026-10-07).**
    - **"No client (personal or passion project)"** tick box under Billed to in New Project, with any template. Ticked (or in a no-client space like Personal, where it's always on): Billed to is hidden, the job code uses your own code (e.g. `OWN-W01`), and **every question is optional**. Templates still have exactly one client field (#7 holds).
    - **Empty answers don't leave gaps in names:** an empty value takes one separator next to it (`_ - . space`), so `{start|yymmdd}_{artist|caps}_{song|caps}` with no artist gives `261007_SONG`. Separators written in the template away from empty values stay. This applies to every project, with or without a client.
    - **Personal folder (v0.4.2, 2026-10-08), optional:** Settings → General → "Personal projects go in" picks one of the jobs folders for Personal-space and "No client" projects (default: the same folder as other new projects). New Project's Location switches to it, and its Change… then changes the Personal folder (adding it to the jobs folders if new). Removing that jobs folder resets the choice. Preview and Create use the same root, so the Plan says where the project goes.
    - **Template editor order:** template name and description at the top, then ① Questions ② Folder name ③ Folders. Questions are one line each (label, kind, Required switch, ▸ details, ×); "+ Add a question" lists the kinds with examples. The folder name is built by clicking answers; the list title and "write it as text" are under More options. "New template…" starts empty or from a copy of any template.

## 12. Out of scope for v1 (do not build)

- Resolve/Premiere/AE project creation
- Drop-to-rename, export naming, watch folders
- ops app or any connection
- Delivery check, checksums (archive moved to §12a #21 on 2026-10-04)
- Licensing, payments, accounts, telemetry
- Any deletion of user files (templates excepted: your own go to the Recycle Bin / Trash, see §12a #16)
