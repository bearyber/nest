import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { detectPlatform } from "./lib/platform";
import "./styles/tokens.css";
import "./styles/base.css";

document.documentElement.dataset.platform = detectPlatform(navigator.userAgent);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
