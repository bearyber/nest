import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { detectPlatform } from "./lib/platform";
import { installBrowserKeyGuard } from "./lib/shortcuts";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/motion.css";

const platform = detectPlatform(navigator.userAgent);
document.documentElement.dataset.platform = platform;
// Windows: the webview's own F5 / Ctrl+R reload, Ctrl+F find, Ctrl+P print and zoom never run, in
// every window (main, first run, Settings). WebView2's zoom is also off in the window config.
installBrowserKeyGuard(window, platform);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
