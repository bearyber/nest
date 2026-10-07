export type Platform = "mac" | "win";

// Tokens differ per OS (build spec §8). Anything that isn't macOS gets Windows tokens.
export function detectPlatform(userAgent: string): Platform {
  return /Mac OS X|Macintosh/.test(userAgent) ? "mac" : "win";
}

/** Cmd on macOS, Ctrl elsewhere. Reads the platform set on <html> at startup. */
export const isMac = () => document.documentElement.dataset.platform === "mac";

/** Whether the OS "command" modifier (Cmd / Ctrl) is held. */
export const hasCommandKey = (e: KeyboardEvent) => (isMac() ? e.metaKey : e.ctrlKey);
