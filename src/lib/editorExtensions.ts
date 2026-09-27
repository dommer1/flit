// The Tiptap extensions of the compose editor, shared by the component and
// its tests.
import Image from "@tiptap/extension-image";
import StarterKit from "@tiptap/starter-kit";

// SECURITY: the compose editor may hold an expanded reply-quote — images
// are limited to the message's own inline forms (data:/cid:). A remote
// URL would be a network load in the app's main frame, so it is rejected
// at parse time and the node is dropped.
const InlineImage = Image.extend({
  parseHTML() {
    return [
      {
        tag: "img[src]",
        getAttrs: (element) =>
          /^(data:image\/|cid:)/i.test(element.getAttribute("src") ?? "")
            ? null
            : false,
      },
    ];
  },
});

export const EXTENSIONS = [StarterKit, InlineImage];
