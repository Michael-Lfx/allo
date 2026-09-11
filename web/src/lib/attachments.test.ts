import { describe, expect, it } from "vitest";

import {
  MAX_ATTACHMENTS,
  attachmentCandidates,
  attachmentExtension,
  attachmentName,
  classifyAttachment,
} from "./attachments";
import type { WorkspaceFlatFile } from "./protocol";

function file(name: string): WorkspaceFlatFile {
  return { name, full_path: `C:/ws/${name}`, relative_path: name };
}

describe("attachmentExtension / attachmentName", () => {
  it("大小写无关，Windows 反斜杠也算路径", () => {
    expect(attachmentExtension("C:\\ws\\Shot.PNG")).toBe("png");
    expect(attachmentExtension("/ws/a.jpeg")).toBe("jpeg");
    expect(attachmentExtension("dir.d/file")).toBe("");
    expect(attachmentName("C:\\ws\\sub\\shot.png")).toBe("shot.png");
  });
});

describe("classifyAttachment", () => {
  it("运行时能送进模型的格式才算 supported", () => {
    for (const name of ["a.png", "a.jpg", "a.jpeg", "a.webp", "A.PNG"]) {
      expect(classifyAttachment(name).kind, name).toBe("supported");
    }
  });

  it("运行时明确拒绝的图片格式单独一类并带扩展名（不是静默丢弃）", () => {
    for (const name of ["a.gif", "a.bmp", "a.svg", "a.heic", "a.tiff"]) {
      const entry = classifyAttachment(name);
      expect(entry.kind, name).toBe("rejected-image");
      if (entry.kind === "rejected-image") {
        expect(entry.extension).toBe(attachmentExtension(name));
      }
    }
  });

  it("非图片是第三类（运行时直接忽略，界面不提供）", () => {
    expect(classifyAttachment("/ws/notes.md").kind).toBe("not-an-image");
    expect(classifyAttachment("/ws/Makefile").kind).toBe("not-an-image");
  });
});

describe("attachmentCandidates", () => {
  it("分流可附 / 不可附，已选中的不再出现", () => {
    const { pickable, skipped, remaining } = attachmentCandidates(
      [file("a.png"), file("b.gif"), file("c.md"), file("d.webp")],
      ["C:/ws/d.webp"],
    );
    expect(pickable.map((entry) => entry.name)).toEqual(["a.png"]);
    expect(skipped.map((entry) => entry.name)).toEqual(["b.gif", "c.md"]);
    expect(remaining).toBe(MAX_ATTACHMENTS - 1);
  });

  it("名额按已选数量收缩，满了就没有可选项", () => {
    const picked = Array.from({ length: MAX_ATTACHMENTS }, (_, index) => `C:/ws/p${index}.png`);
    const { pickable, remaining } = attachmentCandidates([file("a.png")], picked);
    expect(remaining).toBe(0);
    expect(pickable).toEqual([]);
  });

  it("超出剩余名额的可选项被截断（而不是全部列出再失败）", () => {
    const picked = Array.from({ length: MAX_ATTACHMENTS - 1 }, (_, index) => `C:/ws/p${index}.png`);
    const { pickable, remaining } = attachmentCandidates([file("a.png"), file("b.png")], picked);
    expect(remaining).toBe(1);
    expect(pickable.map((entry) => entry.name)).toEqual(["a.png"]);
  });
});
