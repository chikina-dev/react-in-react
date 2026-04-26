import { expect, test } from "@playwright/test";

test("launches sample2.zip through the browser app upload flow", async ({ page }) => {
  test.setTimeout(90000);
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
    .setInputFiles("/Users/chikina/workspace/production/node-in-node/sample/sample2.zip");

  await expect(page.getByText(/Mounted .* sample2\.zip into \/workspace\./)).toBeVisible({
    timeout: 30000,
  });

  await page.getByLabel("Args").fill("run start");
  await page.getByRole("button", { name: "Run session" }).click();

  const deadline = Date.now() + 45000;
  for (;;) {
    const snapshot = await page.evaluate(() => {
      const previewCode = document.querySelector(".preview-panel code")?.textContent ?? "";
      const terminal = Array.from(document.querySelectorAll(".terminal .line span:last-child"))
        .map((node) => node.textContent ?? "")
        .slice(-16);

      return {
        previewCode,
        terminal,
      };
    });

    if (snapshot.previewCode.includes("/preview/")) {
      expect(snapshot.previewCode).toContain("/preview/");
      break;
    }

    const terminalText = snapshot.terminal.join(" | ");
    if (
      terminalText.includes("INLINE_RUNTIME_FAILED") ||
      terminalText.includes("RUN_PLAN_FAILED") ||
      terminalText.includes("ENTRYPOINT_NOT_FOUND") ||
      terminalText.includes("SCRIPT_NOT_FOUND")
    ) {
      throw new Error(
        `sample2.zip failed before preview\nterminal=${terminalText}\nconsole=${consoleMessages.join(" | ")}\npageErrors=${pageErrors.join(" | ")}`,
      );
    }

    if (Date.now() >= deadline) {
      throw new Error(
        `sample2.zip did not reach preview\npreview=${snapshot.previewCode}\nterminal=${terminalText}\nconsole=${consoleMessages.join(" | ")}\npageErrors=${pageErrors.join(" | ")}`,
      );
    }

    await page.waitForTimeout(500);
  }

  const previewFrame = page.frameLocator('iframe[title="guest-react-preview"]');
  await expect(previewFrame.getByText("Learn React")).toBeVisible({ timeout: 60000 });
  await expect(previewFrame.getByText("Edit src/App.js and save to reload.")).toBeVisible({
    timeout: 60000,
  });
  const frameHandle = await page.locator('iframe[title="guest-react-preview"]').elementHandle();
  const frame = await frameHandle?.contentFrame();
  let cssState:
    | {
        bodyMargin: string | null;
        headerDisplay: string | null;
        headerBackground: string | null;
        linkColor: string | null;
      }
    | undefined;
  const cssDeadline = Date.now() + 15000;
  for (;;) {
    cssState = await frame?.evaluate(() => {
      const body = document.body;
      const header = document.querySelector<HTMLElement>(".App-header");
      const link = document.querySelector<HTMLElement>(".App-link");
      return {
        bodyMargin: body ? getComputedStyle(body).margin : null,
        headerDisplay: header ? getComputedStyle(header).display : null,
        headerBackground: header ? getComputedStyle(header).backgroundColor : null,
        linkColor: link ? getComputedStyle(link).color : null,
      };
    });
    if (cssState?.headerDisplay === "flex") {
      break;
    }
    if (Date.now() >= cssDeadline) {
      break;
    }
    await page.waitForTimeout(250);
  }
  expect(cssState?.bodyMargin).toBe("0px");
  expect(cssState?.headerDisplay).toBe("flex");
  expect(cssState?.headerBackground).toBe("rgb(40, 44, 52)");
  expect(cssState?.linkColor).toBe("rgb(97, 218, 251)");
  const html = await frame?.evaluate(() => document.documentElement.outerHTML);
  expect(html).not.toContain("%PUBLIC_URL%");
});
