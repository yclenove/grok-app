/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import "@/test/jsdomStubs";
import {
  MarkdownTiptapEditor,
  type MarkdownTiptapLabels,
} from "./MarkdownTiptapEditor";

const labels: MarkdownTiptapLabels = {
  bold: "Bold",
  italic: "Italic",
  strike: "Strike",
  code: "Code",
  h1: "Heading 1",
  h2: "Heading 2",
  h3: "Heading 3",
  bulletList: "Bullet list",
  orderedList: "Ordered list",
  blockquote: "Blockquote",
  link: "Link",
  hr: "Separator",
  linkPlaceholder: "URL",
  linkApply: "Apply",
  placeholder: "Write Markdown",
  editorAria: "Markdown editor",
};

afterEach(cleanup);

describe("MarkdownTiptapEditor patched parser", () => {
  it("preserves formatting, linkification and literal code in the real editor", async () => {
    const { container } = render(
      <MarkdownTiptapEditor
        value={"# Heading\n\n**Bold**\n\n- First\n- Second\n\nhttps://example.com/docs\n\na@b.co\n\n`a@b.co`"}
        onChange={vi.fn()}
        labels={labels}
      />,
    );
    await waitFor(() => {
      expect(container.querySelector(".tiptap h1")?.textContent).toBe("Heading");
    });
    expect(container.querySelector(".tiptap strong")?.textContent).toBe("Bold");
    expect(container.querySelectorAll(".tiptap ul li")).toHaveLength(2);
    expect(container.querySelector('a[href="https://example.com/docs"]')).not.toBeNull();
    expect(container.querySelectorAll('a[href="mailto:a@b.co"]')).toHaveLength(1);
    expect(container.querySelector(".tiptap code")?.textContent).toBe("a@b.co");
    expect(container.querySelector(".tiptap code a")).toBeNull();
  });

  it("reloads external Markdown, keeps save handling and supports read-only mode", async () => {
    const onSave = vi.fn();
    const onChange = vi.fn();
    const props = { onSave, onChange, labels };
    const { container, getByLabelText, rerender } = render(
      <MarkdownTiptapEditor {...props} value="Original" />,
    );
    await waitFor(() => expect(container.querySelector(".tiptap")).not.toBeNull());
    fireEvent.keyDown(getByLabelText(labels.editorAria, { selector: ".tiptap" }), {
      key: "s",
      ctrlKey: true,
    });
    expect(onSave).toHaveBeenCalledOnce();

    rerender(
      <MarkdownTiptapEditor {...props} value={"## Reloaded\n\n**New**"} disabled />,
    );
    await waitFor(() => {
      expect(container.querySelector(".tiptap h2")?.textContent).toBe("Reloaded");
    });
    expect(container.querySelector(".tiptap strong")?.textContent).toBe("New");
    expect(
      getByLabelText(labels.editorAria, { selector: ".tiptap" }).getAttribute("contenteditable"),
    ).toBe("false");
  });
});
