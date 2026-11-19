import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/geist";
import "./index.css";
import { applyTheme, getSavedThemeId } from "./lib/themes";
import { App } from "./App";

applyTheme(getSavedThemeId());

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
);
