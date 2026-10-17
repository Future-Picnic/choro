import "@openuidev/react-ui/defaults.css";
import "@openuidev/react-ui/styles/table.css";
import "@openuidev/react-ui/styles/charts.css";
import "@openuidev/react-ui/styles/input.css";
import "@openuidev/react-ui/styles/iconButton.css";
import "./style.css";
import { createRoot } from "react-dom/client";
import { App } from "./App";

const boot = window.__CHORO_VISUAL__;
const root = document.getElementById("root");
if (root && boot) {
  createRoot(root).render(<App boot={boot} />);
} else if (root) {
  root.textContent = "This visual answer is unavailable.";
}
