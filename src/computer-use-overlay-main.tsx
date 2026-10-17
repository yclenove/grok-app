import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ComputerUseOverlayApp } from "./components/computer-use/ComputerUseOverlay";
import "./styles/tokens.css";
document.documentElement.dataset.cuOverlay = "1";
document.addEventListener("contextmenu", (event) => event.preventDefault());
createRoot(document.getElementById("root")!).render(<StrictMode><ComputerUseOverlayApp /></StrictMode>);
