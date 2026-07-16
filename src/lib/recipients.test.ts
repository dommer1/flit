import { describe, expect, it } from "vitest";
import { activeTerm, applySuggestion } from "./recipients";

describe("activeTerm", () => {
  it("is the whole value while typing the first recipient", () => {
    expect(activeTerm("", 0)).toBe("");
    expect(activeTerm("ann", 3)).toBe("ann");
  });

  it("is the segment after the last comma before the caret", () => {
    expect(activeTerm("a@x.com, bo", 11)).toBe("bo");
    expect(activeTerm("a@x.com,bo", 10)).toBe("bo");
  });

  it("ignores recipients after the caret", () => {
    // Caret inside "bob" of "ann, bob, carl".
    expect(activeTerm("ann, bob, carl", 7)).toBe("bo");
  });
});

describe("applySuggestion", () => {
  it("replaces a lone term with the address", () => {
    expect(applySuggestion("bo", 2, "bob@x.com")).toBe("bob@x.com");
  });

  it("replaces only the term being typed, keeping earlier recipients", () => {
    expect(applySuggestion("a@x.com, bo", 11, "bob@y.sk")).toBe(
      "a@x.com, bob@y.sk",
    );
  });

  it("keeps recipients after the replaced term", () => {
    expect(applySuggestion("ann, bo, carl", 7, "bob@x.com")).toBe(
      "ann, bob@x.com, carl",
    );
  });
});
