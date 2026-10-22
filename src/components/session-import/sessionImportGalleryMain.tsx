import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../../styles/tokens.css";
import "../../styles/skins.css";
import "../../styles/tailwind.css";
import "../../styles/app.css";
import "../../styles/setup-wizard.css";
import { SessionImportReviewGallery } from "./SessionImportReviewGallery";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <SessionImportReviewGallery />
    </StrictMode>,
  );
}
