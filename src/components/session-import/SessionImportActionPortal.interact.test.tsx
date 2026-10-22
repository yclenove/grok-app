/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { GlassModal } from "@/components/GlassModal";
import { createT } from "@/i18n";
import { SessionImportMappingStep } from "./SessionImportMappingStep";
import { useSessionImportAssociations } from "@/hooks/useSessionImportAssociations";

vi.mock("@/lib/nativeWebviewCover", () => ({ acquireNativeWebviewCover: () => () => {} }));
afterEach(cleanup);
const tr = createT("en");

function Harness({ onBack, onCommit, covered = false }: {
  onBack: () => void; onCommit: () => void; covered?: boolean;
}) {
  const [target, setTarget] = useState<HTMLDivElement | null>(null);
  const associations = useSessionImportAssociations(null, false, () => false);
  return <>
    <GlassModal open wrapBody onClose={() => {}} title="Synthetic mapping" closeLabel="Close mapping"
      className="session-import-modal session-import-wizard session-import-wizard--mapping"
      footer={<div ref={setTarget} className="session-import__action-host" />}>
      <SessionImportMappingStep tr={tr} footerTarget={target} mappingSessions={[]} mappingSessionCount={0}
        ui={{ busy: false, prepareBatch: null, mappings: [], registrations: [], pendingPlan: null,
          projectId: null, projects: [], setMappingProject: () => {}, commitPhase: null,
          associations, prepareItems: [], prepareNextCursor: null, prepareItemsError: false,
          loadMorePrepare: async () => false, retryPrepareItems: async () => false,
          explicitReimport: false, setExplicitReimport: () => {} }}
        onBack={onBack} commitImport={onCommit} />
    </GlassModal>
    {covered ? <GlassModal open wrapBody onClose={() => {}} title="Synthetic covering dialog" closeLabel="Close cover">
      <textarea aria-label="Cover editor" />
    </GlassModal> : null}
  </>;
}

test("the real portaled mapping footer stays in the same focus loop and Enter activates only its button", async () => {
  const user = userEvent.setup();
  const back = vi.fn(); const commit = vi.fn();
  render(<Harness onBack={back} onCommit={commit} />);
  const dialog = await screen.findByRole("dialog", { name: "Synthetic mapping" });
  const close = within(dialog).getByRole("button", { name: "Close mapping" });
  const next = within(dialog).getByRole("button", { name: tr("sessionImport.importSelected") });
  expect(next.closest(".modal-body")).toBeNull();
  expect(next.closest(".modal-actions")).not.toBeNull();
  await waitFor(() => expect(close).toHaveFocus());
  // The body's re-import choice comes before the portaled footer actions.
  await user.tab();
  expect(within(dialog).getByRole("checkbox", { name: tr("sessionImport.explicitReimport") })).toHaveFocus();
  await user.tab();
  expect(within(dialog).getByRole("button", { name: tr("sessionImport.back") })).toHaveFocus();
  await user.keyboard("{Enter}");
  expect(back).toHaveBeenCalledOnce(); expect(commit).not.toHaveBeenCalled();
  await user.tab(); expect(next).toHaveFocus();
  await user.keyboard("{Enter}");
  expect(commit).toHaveBeenCalledOnce();
  await user.tab(); expect(close).toHaveFocus();
  await user.tab({ shift: true }); expect(next).toHaveFocus();
});

test("a covering dialog owns typing and blocks background mapping activation after actions are portaled", async () => {
  const user = userEvent.setup();
  const back = vi.fn(); const commit = vi.fn();
  render(<Harness onBack={back} onCommit={commit} covered />);
  const cover = await screen.findByRole("dialog", { name: "Synthetic covering dialog" });
  const editor = within(cover).getByRole("textbox", { name: "Cover editor" });
  await user.click(editor); await user.type(editor, "np{Enter}synthetic");
  expect(editor).toHaveValue("np\nsynthetic");
  fireEvent.keyDown(document.querySelector("[data-import-mapping='1']")!, { key: "Enter" });
  expect(back).not.toHaveBeenCalled(); expect(commit).not.toHaveBeenCalled();
  expect(editor).toHaveFocus();
});
