import { describe, expect, it } from "vitest";
import { mergeAttachments, type Attachment } from "./attachments";
import { mapStoredMessageToChat } from "./mapStoredMessages";
import {
  rewindComposerRestore,
  stripUserAttachmentRefs,
} from "./session/rewind";

const windowsPaths = [
  "C:\\Users\\synthetic\\Pictures\\screen shot.png",
  "C:\\Users\\synthetic\\Pictures\\中文 目录\\风景.png",
];

describe("normalized attachments in history and rewind consumers", () => {
  it.each(windowsPaths)(
    "reloads one user image and restores it for editing: %s",
    (path) => {
      const stored: Attachment = { path, name: "chosen image", isDir: false };
      const message = mapStoredMessageToChat({
        id: "synthetic-user",
        role: "user",
        content: `合成附件\n\n@${path}`,
        attachments: [stored],
      });

      expect(message.content).toBe("合成附件");
      expect(message.attachments).toEqual([stored]);
      const stripped = stripUserAttachmentRefs(message);
      expect(stripped).toBe(message);
      expect(rewindComposerRestore([stripped], null)).toEqual({
        text: "合成附件",
        attachments: [stored],
      });
    },
  );

  it.each(windowsPaths)(
    "reloads one assistant image without changing prose: %s",
    (path) => {
      const stored: Attachment = { path, name: "stored metadata", isDir: false };
      const content = `合成图片：\`${path}\``;
      const message = mapStoredMessageToChat({
        id: "synthetic-assistant",
        role: "assistant",
        content,
        attachments: [stored],
      });

      expect(message.content).toBe(content);
      expect(message.attachments).toEqual([stored]);
      expect(stripUserAttachmentRefs(message)).toBe(message);
    },
  );

  it("deduplicates separators in a raw user row before rewind restoration", () => {
    const path = windowsPaths[1]!;
    const alias = path.replace(/\\/g, "/");
    const stripped = stripUserAttachmentRefs({
      id: "synthetic-raw-user",
      role: "user",
      content: `修改这张图\n\n@${path}`,
      attachments: [{ path: alias, name: "风景.png", isDir: false }],
    });

    expect(stripped.content).toBe("修改这张图");
    expect(stripped.attachments).toEqual([
      { path, name: "风景.png", isDir: false },
    ]);
    expect(rewindComposerRestore([stripped], null)).toEqual({
      text: "修改这张图",
      attachments: stripped.attachments,
    });
  });

  it("keeps case-sensitive POSIX files and distinct remote URLs separate", () => {
    const paths = [
      "/tmp/synthetic/Image.png",
      "/tmp/synthetic/image.png",
      "https://example.invalid/Image.png?version=1",
      "https://example.invalid/image.png?version=1",
      "https://example.invalid/Image.png?version=2",
    ];
    const attachments = paths.map((path) => ({
      path,
      name: path,
      isDir: false,
    }));
    expect(mergeAttachments(attachments, attachments)).toEqual(attachments);
  });
});
