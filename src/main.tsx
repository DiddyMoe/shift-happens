import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { StartupGate } from "./StartupGate";
import { applyTheme, rememberedTheme } from "./lib/theme";
import "./styles/index.css";

const root = document.getElementById("root");
if (!root) {
  throw new Error("The application root is missing.");
}

applyTheme(rememberedTheme());
createRoot(root).render(
  <StrictMode>
    <StartupGate />
  </StrictMode>,
);
