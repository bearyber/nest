# Nest (by INTRVL)

Desktop app for Windows + macOS that creates project folders, starter files and job codes from templates. Tauri 2 (Rust core) + React/TypeScript UI.

Keep changes small and explain them in plain words. Never leave the gate red.

Source of truth for behaviour: `docs/V1-BUILD-SPEC.md`.

Private notes (how we work, releasing, TODO, SHIPLOG, plans) live in `private/`, a separate private repo the public one ignores:

@private/WORKING.md

## Locked decisions (do not change without the owner saying so)

1. **Both OSes or nothing.** Every feature works on Windows 10/11 and macOS 13+. No `#[cfg]`-only features without the other OS covered.
2. **All filesystem writes in Rust.** The webview never writes files. No fs write permissions in `capabilities/`.
3. **Plan, then apply.** `plan_project` is pure and returns what will be created. The preview renders the Plan. `apply_plan` executes it. Never create anything that isn't in the Plan.
4. **Never delete or overwrite user files.** Only rollback of items created in the same failed run. No "cleanup" features. *Exception (Bernard, 2026-10-02): your own templates may go to the Recycle Bin / Trash (delete, go back to original, replaced by a newer save), always restorable. Never projects.*
5. **The folder is the truth.** `.project.json` is canonical; SQLite index is a disposable cache. Preserve unknown manifest fields on every write.
6. **Atomic JSON writes** (tmp + fsync + rename). Always.
7. **Native feel.** System fonts, system accent, native menus/context menus/dialogs, no web-looking widgets, no component libraries, no Tailwind. Tokens in `src/styles/tokens.css`.
8. **Sanitise every generated name** through the one sanitiser in `src-tauri/src/names.rs`. Never build a path from raw user input anywhere else.
9. **Standalone.** No network calls except the updater. No accounts, telemetry or connections in v1.
10. **Scope is v1 only.** Anything in build spec section 12 is out. If a task drifts there, stop and ask.

## Layout

```
src/                 React UI
  components/        presentational pieces
  screens/           Main, NewProjectSheet, Settings, FirstRun
  lib/               typed wrappers around Tauri commands (invoke)
  styles/tokens.css  design tokens (build spec section 8)
src-tauri/
  src/
    commands.rs      Tauri command handlers (thin; call into modules)
    template.rs      template parsing + validation
    template_io.rs   installed templates: load, import/export (.nesttemplate), duplicate, delete to bin
    settings_cmd.rs  Settings window + first-run commands (one `edit` path, `settings-changed` event)
    update.rs        auto-update (release builds)
    plan.rs          plan_project (pure)
    apply.rs         apply_plan + rollback
    names.rs         sanitiser + job codes
    manifest.rs      .project.json read/write (atomic, preserve unknown)
    index.rs         SQLite index + rescan
    devices.rs       Devices: this computer's list, reading other computers' lists, Google Drive detection
    devices_cmd.rs   Devices commands + background sync (launch, every minute, focus, after changes, quit)
    template_edit.rs template editor: preview, save (Rust picks id/version), one copy per template (T-A)
    settings.rs      settings file
    sizes.rs         folder sizes per subfolder and file type (read-only walk; cached in the index)
    error.rs         AppError (user-facing message)
    menu.rs          macOS menu bar (Windows has none: shortcuts + context menus)
    window.rs        window appearance (Mica / vibrancy, theme)
  templates/         bundled templates
  tests/             integration tests (temp dirs only)
docs/                V1-BUILD-SPEC.md
private/             (not in this repo) private notes, own templates; see private/WORKING.md
```

## Conventions

- Tauri commands return `Result<T, AppError>`; `AppError` has a user-facing message. The UI shows that message, never a raw error string or stack.
- Paths: `PathBuf` in Rust, strings only for display. Template paths always use `/`; convert in Rust.
- Types shared UI ↔ Rust are defined once in Rust with `serde` and mirrored in `src/lib/types.ts`. Update both in the same change.
- Hidden manifest: dot-prefix on both OSes + hidden attribute on Windows.
- Unicode: NFC-normalise all names. Test with Korean and Chinese strings.
- Dev builds use a `NestDev` jobs root. Tests use temp dirs. **Never touch a real jobs folder in tests.**
- Run the dev app with `npm run app` (its own webview folder, so it runs next to the installed Nest on Windows). `src-tauri/tauri.dev.conf.json` copies the main window from `tauri.conf.json`: change both together.

## Gate (run before saying a task is done)

```
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npm run typecheck && npm run lint && npm test
```

If you can't run something on the other OS, say so explicitly and list what to check by hand on that machine.
