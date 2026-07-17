import { expect, it } from "vitest";
import { extColor, fileExt } from "./attachments";

it("extracts the uppercased extension, capped at four characters", () => {
  expect(fileExt("potvrdenie-zmeny-ns.pdf")).toBe("PDF");
  expect(fileExt("dns-zaznamy.csv")).toBe("CSV");
  expect(fileExt("archive.tar.gz")).toBe("GZ");
  expect(fileExt("report.numbers")).toBe("NUMB");
});

it("returns an empty extension for files without one", () => {
  expect(fileExt("README")).toBe("");
  expect(fileExt(".gitignore")).toBe("");
  expect(fileExt("trailing.")).toBe("");
});

it("maps known extensions to their tile color", () => {
  expect(extColor("PDF")).toBe("#e0443e");
  expect(extColor("CSV")).toBe("#34c759");
  expect(extColor("XLSX")).toBe("#34c759");
  expect(extColor("DOCX")).toBe("#0a66c2");
  expect(extColor("PNG")).toBe("#7a5cc2");
});

it("falls back to gray for unknown extensions", () => {
  expect(extColor("XYZ")).toBe("#8e8e93");
  expect(extColor("")).toBe("#8e8e93");
});
