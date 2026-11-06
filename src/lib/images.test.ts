import { describe, expect, it } from "vitest";
import { altText, isImageFile } from "./images";

describe("isImageFile", () => {
  it("accepts image MIME types and image extensions", () => {
    expect(isImageFile({ name: "a", type: "image/png" })).toBe(true);
    expect(isImageFile({ name: "shot.JPG", type: "" })).toBe(true);
    expect(isImageFile({ name: "d.svg", type: "" })).toBe(true);
  });

  it("rejects everything else", () => {
    expect(isImageFile({ name: "notes.md", type: "text/markdown" })).toBe(false);
    expect(isImageFile({ name: "run.exe", type: "application/octet-stream" })).toBe(false);
    expect(isImageFile({ name: "png", type: "" })).toBe(false);
  });
});

describe("altText", () => {
  it("uses the file name without extension and strips brackets", () => {
    expect(altText("Screenshot 2026.png")).toBe("Screenshot 2026");
    expect(altText("a[1].jpg")).toBe("a1");
    expect(altText("archive.tar.gz")).toBe("archive.tar");
  });

  it("falls back to 'image'", () => {
    expect(altText("")).toBe("image");
    expect(altText(".png")).toBe("image");
    expect(altText("[].png")).toBe("image");
  });
});
