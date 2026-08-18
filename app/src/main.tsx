import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./ui/tokens.css";
import "./styles.css";

const root = document.getElementById("root");
if (root === null) {
  throw new Error("P2P Desk root element is missing");
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
