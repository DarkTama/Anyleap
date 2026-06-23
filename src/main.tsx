import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ControlWindow } from "./components/ControlWindow";
import "./index.css";

// The floating control window loads the same bundle with ?control=1&serial=…
const params = new URLSearchParams(window.location.search);
const controlSerial = params.get("control") ? params.get("serial") : null;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {controlSerial ? <ControlWindow serial={controlSerial} /> : <App />}
  </React.StrictMode>,
);
