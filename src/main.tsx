import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ControlWindow } from "./components/ControlWindow";
import "./index.css";

// The floating control window loads the same bundle with ?control=1&serial=…
const params = new URLSearchParams(window.location.search);
const controlSerial = params.get("control") ? params.get("serial") : null;

// The control window is transparent so the collapsed round button has no
// square backdrop; the page background must not paint over it.
if (controlSerial) {
  document.documentElement.style.background = "transparent";
  document.body.style.background = "transparent";
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {controlSerial ? <ControlWindow serial={controlSerial} /> : <App />}
  </React.StrictMode>,
);
