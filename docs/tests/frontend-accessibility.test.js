"use strict";

const { describe, it } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { JSDOM } = require("jsdom");
const axe = require("axe-core");

const frontendPath = path.join(__dirname, "..", "..", "frontend");
const html = fs.readFileSync(path.join(frontendPath, "index.html"), "utf8");
const appScript = fs.readFileSync(path.join(frontendPath, "app.js"), "utf8");

async function assertAccessible({ backendResponse, backendError } = {}) {
  const dom = new JSDOM(html, { url: "http://localhost/", runScripts: "outside-only" });
  const { window } = dom;

  if (backendResponse || backendError) {
    window.fetch = backendError
      ? async () => {
          throw backendError;
        }
      : async () => ({
          json: async () => backendResponse,
        });
    window.eval(appScript);
    await new Promise((resolve) => setTimeout(resolve, 0));
  }

  window.eval(axe.source);
  const { violations } = await window.axe.run(window.document, {
    runOnly: {
      type: "tag",
      values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"],
    },
  });
  window.close();

  assert.deepEqual(
    Array.from(violations, ({ id, impact, help, nodes }) => ({
      id,
      impact,
      help,
      elements: Array.from(nodes, ({ target }) => target),
    })),
    [],
    "Axe found accessibility violations",
  );
}

function relativeLuminance(color) {
  const [red, green, blue] = color.match(/\d+/g).map(Number).map((channel) => {
    const value = channel / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
}

describe("frontend accessibility", () => {
  it("meets the automated baseline while the backend status is loading", async () => {
    await assertAccessible();
  });

  it("meets the automated baseline when the backend is available", async () => {
    await assertAccessible({
      backendResponse: {
        status: "ok",
        service: "karis-ky",
        contracts: ["escrow"],
      },
    });
  });

  it("meets the automated baseline when the backend is unavailable", async () => {
    await assertAccessible({ backendError: new Error("Backend unavailable") });
  });

  it("keeps the page badge text above the AA contrast threshold", () => {
    const dom = new JSDOM(html);
    const badge = dom.window.document.querySelector(".pill");
    const styles = dom.window.getComputedStyle(badge);
    const luminances = [
      relativeLuminance(styles.color),
      relativeLuminance(styles.backgroundColor),
    ].sort((a, b) => b - a);
    const contrast = (luminances[0] + 0.05) / (luminances[1] + 0.05);
    dom.window.close();

    assert.ok(contrast >= 4.5, `Expected at least 4.5:1 contrast, received ${contrast.toFixed(2)}:1`);
  });
});
