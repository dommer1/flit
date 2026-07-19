import { describe, expect, it } from "vitest";
import { joinRecipients, splitRecipients } from "./recipients";

describe("splitRecipients", () => {
  it("splits a comma-separated list and trims each entry", () => {
    expect(splitRecipients("a@x.com, b@y.com,c@z.com")).toEqual([
      "a@x.com",
      "b@y.com",
      "c@z.com",
    ]);
  });

  it("skips empty pieces", () => {
    expect(splitRecipients("")).toEqual([]);
    expect(splitRecipients("a@x.com, , b@y.com,")).toEqual([
      "a@x.com",
      "b@y.com",
    ]);
  });

  it("keeps a comma inside a quoted display name", () => {
    expect(splitRecipients('"Novák, Ján" <jan@x.sk>, b@y.com')).toEqual([
      '"Novák, Ján" <jan@x.sk>',
      "b@y.com",
    ]);
  });
});

describe("joinRecipients", () => {
  it("joins the chips with commas", () => {
    expect(joinRecipients(["a@x.com", "b@y.com"], "")).toBe(
      "a@x.com, b@y.com",
    );
  });

  it("appends the address still being typed", () => {
    expect(joinRecipients(["a@x.com"], "bo")).toBe("a@x.com, bo");
  });

  it("is just the typed address while there are no chips", () => {
    expect(joinRecipients([], "bo")).toBe("bo");
    expect(joinRecipients([], "")).toBe("");
  });
});
