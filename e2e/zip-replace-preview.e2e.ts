import { expect, test } from "@playwright/test";

async function waitForPreviewUrl(page: import("@playwright/test").Page): Promise<string> {
  const deadline = Date.now() + 45000;
  for (;;) {
    const snapshot = await page.evaluate(() => {
      const previewCode = document.querySelector(".preview-panel code")?.textContent ?? "";
      const terminal = Array.from(document.querySelectorAll(".terminal .line span:last-child"))
        .map((node) => node.textContent ?? "")
        .slice(-20);
      return { previewCode, terminal };
    });

    if (
      snapshot.previewCode.includes("/preview/") &&
      !snapshot.previewCode.includes("<session>") &&
      !snapshot.previewCode.includes("<port>")
    ) {
      return snapshot.previewCode;
    }

    const terminalText = snapshot.terminal.join(" | ");
    if (
      terminalText.includes("INLINE_RUNTIME_FAILED") ||
      terminalText.includes("RUN_PLAN_FAILED") ||
      terminalText.includes("ENTRYPOINT_NOT_FOUND") ||
      terminalText.includes("SCRIPT_NOT_FOUND")
    ) {
      throw new Error(`preview failed before ready: ${terminalText}`);
    }

    if (Date.now() >= deadline) {
      throw new Error(`preview did not reach ready: ${terminalText}`);
    }

    await page.waitForTimeout(500);
  }
}

test("replacing the uploaded zip switches the next run to the new guest app", async ({ page }) => {
  test.setTimeout(90000);
  await page.goto("/");

  await page
    .locator('input[type="file"]')
    .setInputFiles("/Users/chikina/workspace/production/node-in-node/sample/sample1.zip");
  await expect(
    page.getByText(/Mounted \d+ files from sample1\.zip into \/workspace\./),
  ).toBeVisible({
    timeout: 30000,
  });
  await page.getByRole("button", { name: "Use npm run dev" }).click();
  await page.getByRole("button", { name: "Run session" }).click();

  const firstPreviewUrl = await waitForPreviewUrl(page);
  expect(firstPreviewUrl).toContain("/preview/");
  const previewFrame = page.frameLocator('iframe[title="guest-react-preview"]');
  await expect(previewFrame.getByRole("heading", { name: "Get started" })).toBeVisible({
    timeout: 30000,
  });

  await page
    .locator('input[type="file"]')
    .setInputFiles("/Users/chikina/workspace/production/node-in-node/sample/sample2.zip");
  await expect(
    page.getByText(/Mounted \d+ files from sample2\.zip into \/workspace\./),
  ).toBeVisible({
    timeout: 30000,
  });
  await expect(page.getByText(/sample1\.zip into \/workspace\./)).toHaveCount(0);

  await page.getByLabel("Args").fill("run start");
  await page.getByRole("button", { name: "Run session" }).click();

  const secondPreviewUrl = await waitForPreviewUrl(page);
  expect(secondPreviewUrl).toContain("/preview/");
  await expect(previewFrame.getByText("Learn React")).toBeVisible({ timeout: 30000 });
  await expect(previewFrame.getByText("Edit src/App.js and save to reload.")).toBeVisible({
    timeout: 60000,
  });
  await expect(previewFrame.getByText("Get started")).toHaveCount(0);
});
