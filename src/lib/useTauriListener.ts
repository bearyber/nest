import { useEffect, useRef } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";

/**
 * Listen to a Tauri event or window callback for as long as the component is there.
 * `subscribe` is called once (again only when `enabled` turns off and on) with a callback and
 * returns the usual promise of an unlisten function, e.g. `(cb) => listen("name", cb)`.
 *
 * Two things this gets right that hand-written effects didn't:
 * - The handler is read through a ref, so a new handler never re-subscribes (re-listening left
 *   a gap where an event was lost).
 * - If the component goes away before the subscribe promise resolves, it unlistens as soon as
 *   it does (otherwise the listener leaks and fires twice).
 */
export function useTauriListener<E = unknown>(
  subscribe: (callback: (event: E) => void) => Promise<UnlistenFn>,
  handler: (event: E) => void,
  enabled = true,
) {
  const handlerRef = useRef(handler);
  const subscribeRef = useRef(subscribe);
  useEffect(() => {
    handlerRef.current = handler;
    subscribeRef.current = subscribe;
  });
  useEffect(() => {
    if (!enabled) return;
    let unlisten: UnlistenFn | undefined;
    let gone = false;
    subscribeRef
      .current((e) => handlerRef.current(e))
      .then((u) => (gone ? u() : (unlisten = u)))
      .catch(() => {});
    return () => {
      gone = true;
      unlisten?.();
    };
  }, [enabled]);
}
