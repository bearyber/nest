import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(() => ({
  plugins: [react()],
  // Keep Rust errors visible; Tauri expects a fixed port.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**", "**/.claude/**"] },
  },
  // Agents' worktrees (.claude/worktrees) hold copies of the tests: never run those.
  test: { exclude: ["**/node_modules/**", "**/dist/**", "**/.claude/**"] },
}));
