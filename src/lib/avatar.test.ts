import { describe, expect, it } from "vitest";
import { avatarColor, senderDomain } from "./avatar";

describe("senderDomain", () => {
  it("takes the domain out of name-addr form", () => {
    expect(senderDomain("Alice <alice@example.com>")).toBe("example.com");
  });

  it("takes the domain out of a bare address", () => {
    expect(senderDomain("alice@example.com")).toBe("example.com");
  });

  it("lowercases the domain so casing can't split one sender in two", () => {
    expect(senderDomain("Alice <Alice@Example.COM>")).toBe("example.com");
  });

  it("uses the address, not an @ inside the display name", () => {
    expect(senderDomain('"alice@work" <alice@example.com>')).toBe(
      "example.com",
    );
  });

  it("returns null when there is no address at all", () => {
    expect(senderDomain("Mailer Daemon")).toBeNull();
    expect(senderDomain("")).toBeNull();
  });

  it("returns null for a malformed address with an empty domain", () => {
    expect(senderDomain("alice@")).toBeNull();
  });
});

describe("avatarColor", () => {
  it("gives one domain the same color every time", () => {
    expect(avatarColor("example.com")).toBe(avatarColor("example.com"));
  });

  it("separates domains that differ only slightly", () => {
    expect(avatarColor("example.com")).not.toBe(avatarColor("example.org"));
  });

  it("builds a color the webview can paint", () => {
    expect(avatarColor("example.com")).toMatch(
      /^oklch\(\d+% [\d.]+ \d+(\.\d+)?deg\)$/,
    );
  });
});
