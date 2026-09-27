import { afterEach, expect, it, vi } from "vitest";
import { Editor } from "@tiptap/core";
import { EXTENSIONS } from "./editorExtensions";

let editor: Editor | undefined;
afterEach(() => editor?.destroy());

it("never follows a link clicked inside the editor", () => {
  // A pasted link keeps its target, and Tiptap's default openOnClick called
  // window.open(href, target) — with target="_top" that navigated the
  // compose window itself to the sender's page, draft and all.
  const open = vi.spyOn(window, "open").mockImplementation(() => null);
  editor = new Editor({
    extensions: EXTENSIONS,
    content:
      '<p><a href="https://phish.example/login" target="_top">sign in</a></p>',
  });
  const link = editor.view.dom.querySelector("a")!;
  const click = new MouseEvent("click", { button: 0 });
  Object.defineProperty(click, "target", { value: link });

  // why someProp: jsdom has no layout, so a synthetic click never reaches
  // ProseMirror's own click detection. This is the same hook it would call.
  editor.view.someProp("handleClick", (handle) =>
    handle(editor!.view, 2, click),
  );

  expect(open).not.toHaveBeenCalled();
  open.mockRestore();
});
