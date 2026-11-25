import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import PhoneApp from "./phone/PhoneApp";
import "./styles/app.css";
import { applyTheme, getTheme } from "./lib/theme";
import { isPhone } from "./lib/phone";

// Before the first paint, so a chosen light/dark look never flashes the other one.
// The phone follows the system look (Apple HIG: no app-specific appearance setting on iOS).
applyTheme(isPhone ? "system" : getTheme());

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {/* The staff iPhone app (phase 5) is the same build with its own layout. */}
    {isPhone ? <PhoneApp /> : <App />}
  </React.StrictMode>,
);
