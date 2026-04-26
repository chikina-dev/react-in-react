import { expect, test } from "@playwright/test";

test("launches sample3.zip through the browser app upload flow", async ({ page }) => {
  const consoleMessages: string[] = [];
  const pageErrors: string[] = [];
  page.on("console", (message) => {
    consoleMessages.push(`[${message.type()}] ${message.text()}`);
  });
  page.on("pageerror", (error) => {
    pageErrors.push(error.message);
  });

  await page.goto("/");
  await page
    .locator('input[type="file"]')
    .setInputFiles("/Users/chikina/workspace/production/node-in-node/sample/sample3.zip");

  await expect(
    page.getByText(/Mounted \d+ files from sample3\.zip into \/workspace\./),
  ).toBeVisible({
    timeout: 30000,
  });
  await page.getByRole("button", { name: "Use npm run dev" }).click();
  await page.getByRole("button", { name: "Run session" }).click();

  const deadline = Date.now() + 60000;
  for (;;) {
    const snapshot = await page.evaluate(() => {
      const previewCode = document.querySelector(".preview-panel code")?.textContent ?? "";
      const terminal = Array.from(document.querySelectorAll(".terminal .line span:last-child"))
        .map((node) => node.textContent ?? "")
        .slice(-30);
      const previewHeading = document.querySelector(".preview-panel h3")?.textContent ?? "";
      const iframe = document.querySelector('iframe[title="guest-react-preview"]');
      return {
        previewCode,
        terminal,
        previewHeading,
        hasIframe: Boolean(iframe),
      };
    });

    const terminalText = snapshot.terminal.join(" | ");
    if (
      terminalText.includes("INLINE_RUNTIME_FAILED") ||
      terminalText.includes("RUN_PLAN_FAILED") ||
      terminalText.includes("ENTRYPOINT_NOT_FOUND") ||
      terminalText.includes("SCRIPT_NOT_FOUND")
    ) {
      throw new Error(
        `sample3.zip failed before preview\npreview=${snapshot.previewCode}\nheading=${snapshot.previewHeading}\niframe=${snapshot.hasIframe}\nterminal=${terminalText}\nconsole=${consoleMessages.join(" | ")}\npageErrors=${pageErrors.join(" | ")}`,
      );
    }

    if (snapshot.previewCode.includes("/preview/") && snapshot.hasIframe) {
      const frame = page.frameLocator('iframe[title="guest-react-preview"]');
      const bodyText =
        (await frame
          .locator("body")
          .textContent()
          .catch(() => "")) ?? "";
      if (bodyText.includes("Preview diagnostics")) {
        throw new Error(
          `sample3.zip rendered preview diagnostics instead of guest app\npreview=${snapshot.previewCode}\nheading=${snapshot.previewHeading}\niframe=${snapshot.hasIframe}\nbody=${bodyText}\nterminal=${terminalText}\nconsole=${consoleMessages.join(" | ")}\npageErrors=${pageErrors.join(" | ")}`,
        );
      }
      if (bodyText.includes("What's next?")) {
        expect(bodyText).toContain("What's next?");
        expect(bodyText).toContain("React Router Docs");
        break;
      }
    }

    if (Date.now() >= deadline) {
      throw new Error(
        `sample3.zip did not reach guest preview\npreview=${snapshot.previewCode}\nheading=${snapshot.previewHeading}\niframe=${snapshot.hasIframe}\nterminal=${terminalText}\nconsole=${consoleMessages.join(" | ")}\npageErrors=${pageErrors.join(" | ")}`,
      );
    }

    await page.waitForTimeout(500);
  }

  const terminalLines = await page
    .locator(".terminal .line span:last-child")
    .evaluateAll((nodes) => nodes.map((node) => node.textContent ?? ""));
  expect(terminalLines).toContainEqual(
    "Runtime host produced a run plan. (commandKind=npm-script, cwd=/workspace, entrypoint=dev, commandLine=npm run dev, envCount=0)",
  );
  expect(terminalLines).toContainEqual(
    expect.stringContaining(
      "Engine context is ready. (engineContextId=quickjs-ng-browser-context:",
    ),
  );
});
