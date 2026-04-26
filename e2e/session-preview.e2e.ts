import { expect, test } from "@playwright/test";

test("launches sample1.zip through the browser app upload flow and serves preview responses through the C VM path", async ({
  page,
}) => {
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
    .setInputFiles("/Users/chikina/workspace/production/node-in-node/sample/sample1.zip");

  await expect(
    page.getByText(/Mounted \d+ files from sample1\.zip into \/workspace\./),
  ).toBeVisible({
    timeout: 30000,
  });
  await page.getByRole("button", { name: "Use npm run dev" }).click();
  await page.getByRole("button", { name: "Run session" }).click();

  const deadline = Date.now() + 45000;
  let previewUrl = "";
  for (;;) {
    const snapshot = await page.evaluate(() => {
      const statusCluster = Array.from(document.querySelectorAll(".status-cluster strong")).map(
        (node) => node.textContent ?? "",
      );
      const previewCode = document.querySelector(".preview-panel code")?.textContent ?? "";
      const terminal = Array.from(document.querySelectorAll(".terminal .line span:last-child"))
        .map((node) => node.textContent ?? "")
        .slice(-20);

      return {
        statusCluster,
        previewCode,
        terminal,
      };
    });

    if (
      snapshot.previewCode.includes("/preview/") &&
      !snapshot.previewCode.includes("<session>") &&
      !snapshot.previewCode.includes("<port>")
    ) {
      previewUrl = snapshot.previewCode;
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
        `sample1.zip failed before preview\nstatus=${snapshot.statusCluster.join(" / ")}\nterminal=${terminalText}\nconsole=${consoleMessages.join(" | ")}\npageErrors=${pageErrors.join(" | ")}`,
      );
    }

    if (Date.now() >= deadline) {
      throw new Error(
        `sample1.zip did not reach preview\nstatus=${snapshot.statusCluster.join(" / ")}\npreview=${snapshot.previewCode}\nterminal=${terminalText}\nconsole=${consoleMessages.join(" | ")}\npageErrors=${pageErrors.join(" | ")}`,
      );
    }

    await page.waitForTimeout(500);
  }

  expect(previewUrl).toContain("/preview/");
  const previewFrame = page.frameLocator('iframe[title="guest-react-preview"]');
  await expect(previewFrame.getByRole("heading", { name: "Get started" })).toBeVisible({
    timeout: 30000,
  });
  await expect(previewFrame.getByRole("button", { name: "Count is 0" })).toBeVisible();
  const frameHandle = await page.locator('iframe[title="guest-react-preview"]').elementHandle();
  const frame = await frameHandle?.contentFrame();
  await expect
    .poll(async () => {
      return frame?.evaluate(() => {
        const root = document.getElementById("root");
        return root ? getComputedStyle(root).display : null;
      });
    })
    .toBe("flex");
  const cssState = await frame?.evaluate(() => {
    const root = document.getElementById("root");
    const button = document.querySelector("button.counter");
    return {
      rootDisplay: root ? getComputedStyle(root).display : null,
      rootBorderLeft: root ? getComputedStyle(root).borderLeftWidth : null,
      buttonBackground: button ? getComputedStyle(button).backgroundColor : null,
    };
  });
  expect(cssState?.rootDisplay).toBe("flex");
  expect(cssState?.rootBorderLeft).toBe("1px");
  expect(cssState?.buttonBackground).toBe("rgba(170, 59, 255, 0.1)");

  const terminalLines = await page
    .locator(".terminal .line span:last-child")
    .evaluateAll((nodes) => nodes.map((node) => node.textContent ?? ""));

  expect(terminalLines).toContainEqual(
    expect.stringContaining("[host] engine=quickjs-ng-browser-c-vm"),
  );
  expect(terminalLines).toContainEqual(
    expect.stringContaining("[engine-context] state=ready pending-jobs=0 bridge-ready=true"),
  );
  expect(terminalLines).toContainEqual(
    "[browser-cli] runtime=browser-dev-server preview=http-server mode=dev",
  );
});
