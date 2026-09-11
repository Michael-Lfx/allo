import { describe, expect, it } from "vitest";

import { changeCount, changeKey, sortChanges, type FileChangeInfo } from "./artifact-changes";

function change(relative: string, operation: FileChangeInfo["operation"] = "modify"): FileChangeInfo {
  return { file_path: `C:/ws/${relative}`, relative_path: relative, operation };
}

describe("artifact-changes（R20b 纯层）", () => {
  it("changeKey 归一化相对路径：反斜杠 / ./ 前缀 / 前导分隔符", () => {
    expect(changeKey(change("src\\main.rs"))).toBe("src/main.rs");
    expect(changeKey(change("./a.md"))).toBe("a.md");
    expect(changeKey(change("/b.txt"))).toBe("b.txt");
  });

  it("sortChanges 按大小写不敏感字母序，且不就地改动入参", () => {
    const input = [change("B.txt"), change("a.txt"), change("C/d.txt")];
    const sorted = sortChanges(input).map((entry) => entry.relative_path);
    expect(sorted).toEqual(["a.txt", "B.txt", "C/d.txt"]);
    // 原数组保持原序（返回的是新数组）。
    expect(input.map((entry) => entry.relative_path)).toEqual(["B.txt", "a.txt", "C/d.txt"]);
  });

  it("changeCount 是待处理 + 已接受之和", () => {
    expect(changeCount({ staged: [change("a")], unstaged: [change("b"), change("c")] })).toBe(3);
    expect(changeCount({ staged: [], unstaged: [] })).toBe(0);
  });
});
