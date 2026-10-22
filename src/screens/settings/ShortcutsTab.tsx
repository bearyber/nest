import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "../../components/ui";
import { setShortcuts, shortcutRecording } from "../../lib/commands";
import {
  currentPlatform,
  describeCombo,
  effectiveCombos,
  formatCombo,
  listedShortcuts,
  planChange,
  planReset,
  recordedCombo,
  type Change,
  type Combo,
  type ShortcutId,
} from "../../lib/shortcuts";
import type { TabProps } from "../Settings";
import styles from "./settings.module.css";

type Swap = Extract<Change, { kind: "swap" }>;

const HINT = "Press the new keys. Esc cancels.";

/** Keys as symbols to look at, and in words for a screen reader. */
function Keys({ combo, platform }: { combo: Combo; platform: ReturnType<typeof currentPlatform> }) {
  return (
    <span role="img" aria-label={describeCombo(combo, platform)}>
      {formatCombo(combo, platform)}
    </span>
  );
}

/** Shortcuts: every shortcut and its keys; click one, press the new keys. Per computer. */
export default function ShortcutsTab({ view, run }: TabProps) {
  const platform = currentPlatform();
  const overrides = view.settings.shortcuts ?? {};
  const rows = listedShortcuts(platform);
  const [recording, setRecording] = useState<ShortcutId | null>(null);
  const [problem, setProblem] = useState<{
    id: ShortcutId;
    text: string;
  } | null>(null);
  const [asking, setAsking] = useState<{ id: ShortcutId; swap: Swap } | null>(null);
  /** The Mac menu bar is emptied while a key is awaited (Rust gives it back). */
  const quiet = useRef(false);

  const focusKeys = (id: ShortcutId) =>
    requestAnimationFrame(() => document.querySelector<HTMLElement>(`[data-keys="${id}"]`)?.focus());

  const stop = useCallback(() => {
    setRecording(null);
    if (quiet.current) {
      quiet.current = false;
      void shortcutRecording(false).catch(() => {});
    }
  }, []);
  // Leaving the tab (or closing Settings) mid-recording gives the menu bar back.
  useEffect(() => stop, [stop]);

  const begin = async (id: ShortcutId) => {
    setProblem(null);
    setAsking(null);
    if (!quiet.current) {
      quiet.current = true;
      await shortcutRecording(true).catch(() => {});
    }
    setRecording(id);
  };

  /** Act on the outcome of a new key (or a reset) for shortcut `id`. */
  const apply = async (id: ShortcutId, change: Change, reset = false) => {
    const label = rows.find((r) => r.id === id)?.label ?? "";
    if (change.kind === "error") {
      setProblem({ id, text: change.message });
      return;
    }
    stop();
    setProblem(null);
    if (change.kind === "swap") {
      setAsking({ id, swap: change });
      return;
    }
    setAsking(null);
    if (change.kind === "save") {
      const now = rows.find((r) => r.id === id);
      const keys = now && effectiveCombos(now, platform, change.overrides)[0];
      const shown = keys ? formatCombo(keys, platform) : null;
      const text = shown ? `${label} ${reset ? "is back to" : "is now"} ${shown}` : `${label} has no keys`;
      await run(setShortcuts(change.overrides), text);
    }
    focusKeys(id);
  };

  // While waiting for keys, every key press is ours: nothing else in the window runs it (the
  // listener is first in line and stops the press), Esc cancels, Tab moves on, clicking away stops.
  useEffect(() => {
    if (!recording) return;
    const id = recording;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Tab" && !e.ctrlKey && !e.metaKey && !e.altKey) {
        stop();
        return;
      }
      e.preventDefault();
      e.stopPropagation();
      if (e.repeat || e.isComposing) return;
      if (e.key === "Escape") {
        stop();
        setProblem(null);
        focusKeys(id);
        return;
      }
      const got = recordedCombo(e, platform);
      if (!got) return;
      if ("error" in got) setProblem({ id, text: got.error });
      else void apply(id, planChange(id, got.combo, platform, overrides));
    };
    // Clicking anywhere but on this shortcut's keys ends the wait (a click elsewhere also ends
    // Mac's WebKit focus on the button, which never fires blur).
    const onPointer = (e: PointerEvent) => {
      if (e.target instanceof Element && e.target.closest(`[data-keys="${id}"]`)) return;
      stop();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("blur", stop);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("blur", stop);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recording, overrides, platform, stop]);

  useEffect(() => {
    if (asking) document.querySelector<HTMLElement>("[data-swap]")?.focus();
  }, [asking]);

  const changed = rows.some((r) => overrides[r.id]);

  return (
    <section className={styles.section}>
      <p id="shortcuts-hint" className={styles.hint}>
        Click a shortcut's keys, then press the new ones. These are for this computer only.{" "}
        {platform === "mac" ? "Help → Keyboard Shortcuts opens this list." : "F1 opens this list from anywhere."}
      </p>
      {/* Always there (empty), so the hint put into it while waiting for keys is announced. */}
      <div role="status" className={styles.srOnly}>
        {recording ? HINT : ""}
      </div>
      <div className={styles.card}>
        {rows.map((s) => {
          const keys: Combo[] = effectiveCombos(s, platform, overrides);
          const main = keys[0];
          const text = main ? formatCombo(main, platform) : "Not set";
          const extras = keys.slice(1);
          const isChanged = !!overrides[s.id];
          const waiting = recording === s.id;
          const base = s[platform][0];
          return (
            <div key={s.id} className={`${styles.row} ${styles.rowWrap}`}>
              <div className={styles.grow}>
                <div>{s.label}</div>
                {extras.length > 0 && (
                  <div className={styles.sub}>
                    Also works:{" "}
                    {extras.map((c, i) => (
                      <span key={i}>
                        {i > 0 && ", "}
                        <Keys combo={c} platform={platform} />
                      </span>
                    ))}
                  </div>
                )}
                {waiting && (
                  <div className={styles.sub} aria-hidden="true">
                    {HINT}
                  </div>
                )}
                {problem?.id === s.id && (
                  <div className={styles.problem} role="alert">
                    {problem.text}
                  </div>
                )}
              </div>
              <Button
                data-keys={s.id}
                className={waiting ? styles.keysWaiting : styles.keys}
                aria-label={
                  waiting
                    ? `${s.label}: type the new keys`
                    : `${s.label}: ${main ? describeCombo(main, platform) : "no keys"}`
                }
                aria-describedby="shortcuts-hint"
                onClick={() => void (waiting ? stop() : begin(s.id))}
              >
                {waiting ? "Type the new keys…" : text}
              </Button>
              {isChanged && (
                <Button
                  aria-label={`Reset ${s.label}${base ? ` to ${describeCombo(base, platform)}` : ""}`}
                  onClick={() => {
                    stop();
                    void apply(s.id, planReset(s.id, platform, overrides), true);
                  }}
                >
                  Reset
                </Button>
              )}
              {asking?.id === s.id && (
                <div
                  className={styles.ask}
                  role="group"
                  aria-labelledby={`swap-${s.id}`}
                  onKeyDown={(e) => {
                    if (e.key !== "Escape") return;
                    // Settings (Windows) closes on Escape unless something inside used it.
                    e.preventDefault();
                    setAsking(null);
                    focusKeys(s.id);
                  }}
                >
                  <span id={`swap-${s.id}`} role="alert">
                    {asking.swap.message}
                  </span>
                  <Button
                    data-swap=""
                    variant="primary"
                    onClick={() => {
                      const { overrides: next } = asking.swap;
                      setAsking(null);
                      void run(setShortcuts(next), `${s.label} and ${asking.swap.with.label} swapped keys`).then(() =>
                        focusKeys(s.id),
                      );
                    }}
                  >
                    Swap
                  </Button>
                  <Button
                    onClick={() => {
                      setAsking(null);
                      focusKeys(s.id);
                    }}
                  >
                    Cancel
                  </Button>
                </div>
              )}
            </div>
          );
        })}
      </div>
      {changed && (
        <div className={styles.actions}>
          <Button
            onClick={() => {
              stop();
              setAsking(null);
              setProblem(null);
              void run(setShortcuts({}), "All shortcuts are back to the standard keys");
            }}
          >
            Reset All
          </Button>
        </div>
      )}
    </section>
  );
}
