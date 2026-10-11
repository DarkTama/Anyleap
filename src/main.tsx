import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ControlWindow } from "./components/ControlWindow";
import { EmbeddedMirrorWindow } from "./components/EmbeddedMirrorWindow";
import { DesktopHudWindow } from "./components/desktop/DesktopHudWindow";
import type { SessionMode } from "./lib/types";
import "./index.css";

// The floating control window loads the same bundle with ?control=1&serial=…
const params = new URLSearchParams(window.location.search);
const controlSerial = params.get("control") ? params.get("serial") : null;
const mirrorSerial = params.get("mirror") ? params.get("serial") : null;
const mirrorMode = (params.get("mode") as SessionMode) || undefined;
const isHudWindow = params.get("kvm_hud") === "1";

// Transparent background for floating windows
if (controlSerial || isHudWindow) {
  document.documentElement.style.background = "transparent";
  document.body.style.background = "transparent";
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isHudWindow ? (
      <DesktopHudWindow />
    ) : mirrorSerial ? (
      <EmbeddedMirrorWindow serial={mirrorSerial} initialMode={mirrorMode} />
    ) : controlSerial ? (
      <ControlWindow serial={controlSerial} />
    ) : (
      <App />
    )}
  </React.StrictMode>,
);
