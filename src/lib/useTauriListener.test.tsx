// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, renderHook } from "@testing-library/react";
import { useTauriListener } from "./useTauriListener";

afterEach(cleanup);

/** A fake `listen`: remembers the callback, resolves when told to. */
function fake() {
  const unlisten = vi.fn();
  let cb: ((e: string) => void) | undefined;
  let resolve!: (u: () => void) => void;
  const subscribe = vi.fn((c: (e: string) => void) => {
    cb = c;
    return new Promise<() => void>((r) => (resolve = r));
  });
  return { subscribe, unlisten, emit: (e: string) => cb?.(e), resolve: () => resolve(unlisten) };
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("useTauriListener", () => {
  it("does not resubscribe when the handler changes, and events reach the newest handler", async () => {
    const f = fake();
    const a = vi.fn();
    const b = vi.fn();
    const { rerender } = renderHook(({ h }) => useTauriListener<string>(f.subscribe, h), { initialProps: { h: a } });
    f.resolve();
    await flush();
    f.emit("one");
    rerender({ h: b });
    f.emit("two");
    expect(f.subscribe).toHaveBeenCalledTimes(1);
    expect(a).toHaveBeenCalledWith("one");
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledWith("two");
    expect(f.unlisten).not.toHaveBeenCalled();
  });

  it("unlistens when cleanup runs before the subscribe promise resolves", async () => {
    const f = fake();
    const { unmount } = renderHook(() => useTauriListener<string>(f.subscribe, () => {}));
    unmount();
    expect(f.unlisten).not.toHaveBeenCalled();
    f.resolve();
    await flush();
    expect(f.unlisten).toHaveBeenCalledTimes(1);
  });

  it("unlistens on unmount after it resolved", async () => {
    const f = fake();
    const { unmount } = renderHook(() => useTauriListener<string>(f.subscribe, () => {}));
    f.resolve();
    await flush();
    unmount();
    expect(f.unlisten).toHaveBeenCalledTimes(1);
  });

  it("follows `enabled`", async () => {
    const f = fake();
    const { rerender } = renderHook(({ on }) => useTauriListener<string>(f.subscribe, () => {}, on), {
      initialProps: { on: false },
    });
    expect(f.subscribe).not.toHaveBeenCalled();
    rerender({ on: true });
    f.resolve();
    await flush();
    expect(f.subscribe).toHaveBeenCalledTimes(1);
    rerender({ on: false });
    expect(f.unlisten).toHaveBeenCalledTimes(1);
  });
});
