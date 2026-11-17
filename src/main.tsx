import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ControlWindow } from "./components/ControlWindow";
import { EmbeddedMirrorWindow } from "./components/EmbeddedMirrorWindow";
import { DesktopHudWindow } from "./components/desktop/DesktopHudWindow";
import { TransferBarWindow } from "./components/desktop/TransferBarWindow";
import type { SessionMode } from "./lib/types";
import "./index.css";

// The floating control window loads the same bundle with ?control=1&serial=…
const params = new URLSearchParams(window.location.search);
const controlSerial = params.get("control") ? params.get("serial") : null;
const mirrorSerial = params.get("mirror") ? params.get("serial") : null;
const controlFlex = params.get("flex") === "1";
const mirrorMode = (params.get("mode") as SessionMode) || undefined;
const isHudWindow = params.get("kvm_hud") === "1";
const isTransferBarWindow = params.get("kvm_transfer_bar") === "1";

// Transparent background for floating windows
if (controlSerial || isHudWindow || isTransferBarWindow) {
  document.documentElement.style.background = "transparent";
  document.body.style.background = "transparent";
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isTransferBarWindow ? (
      <TransferBarWindow />
    ) : isHudWindow ? (
      <DesktopHudWindow />
    ) : mirrorSerial ? (
      <EmbeddedMirrorWindow serial={mirrorSerial} initialMode={mirrorMode} />
    ) : controlSerial ? (
      <ControlWindow serial={controlSerial} flexDisplay={controlFlex} />
    ) : (
      <App />
    )}
  </React.StrictMode>,
);
