import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import "./styles.css";

const label = getCurrentWindow().label;
document.documentElement.dataset.window = label;
document.documentElement.dataset.platform = navigator.userAgent.includes("Linux")
  ? "linux"
  : navigator.userAgent.includes("Windows")
    ? "windows"
    : "macos";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App windowLabel={label} />
  </React.StrictMode>,
);
