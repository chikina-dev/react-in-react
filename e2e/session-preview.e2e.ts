import { expect, test } from "@playwright/test";

const sampleZipPath = "sample/sample1.zip";

test("uploads sample1.zip, runs the session, and renders the guest React preview", async ({
  page,
}) => {
  await page.goto("/");

  await page.locator('input[type="file"]').setInputFiles(sampleZipPath);

  await expect(page.getByText("sample1.zip")).toBeVisible();

  await page.getByRole("button", { name: "Run session" }).click();

  await expect(page.getByText(/\[mount\].*\/workspace/)).toBeVisible();
  await expect(page.getByText(/\[exec\] npm run dev/)).toBeVisible();
  await expect(page.getByText(/Preview mapped to \/preview\//)).toBeVisible();
  await expect(page.locator('iframe[title="guest-react-preview"]')).toBeVisible();

  const frame = page.frameLocator('iframe[title="guest-react-preview"]');

  await expect(frame.getByRole("heading", { name: "Get started" })).toBeVisible();
  await expect(frame.getByRole("button", { name: "Count is 0" })).toBeVisible();
});
