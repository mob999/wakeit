// Invoked only by the opt-in private Pi acceptance fixture. No screenshots or credentials are printed.
import fs from "node:fs";
import { chromium } from "../web/node_modules/playwright-core/index.mjs";
const fixture = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage({
    viewport: { width: 1440, height: 900 },
    locale: "en-US",
  });
  await page.request.post(fixture.base + "/api/v1/session", {
    data: { username: "operator", password: fixture.password },
    headers: { "X-Wakeit-Request": "1" },
  });
  await page.goto(
    `${fixture.base}/w/${fixture.workspace}/runs?run=${fixture.run}`,
  );
  await page
    .getByRole("button", { name: "Resume session", exact: true })
    .click();
  const modal = page.getByRole("dialog", {
    name: "Resume session",
    exact: true,
  });
  await modal
    .locator(".session-terminal[data-status=connected]")
    .waitFor({ timeout: 30000 });
  await page.waitForFunction(
    () =>
      document
        .querySelector(".session-terminal")
        ?.textContent?.includes("TERMINAL_NATIVE_READY"),
    undefined,
    { timeout: 20000 },
  );
  await page.waitForTimeout(1500);
  await modal.locator(".xterm-helper-textarea").focus();
  await page.keyboard.type("Reply exactly TERMINAL_NATIVE_CONTINUED.", { delay: 50 });
  await page.waitForTimeout(300);
  await page.keyboard.press("Enter");
  await page.waitForFunction(
    () =>
      (
        document
          .querySelector(".session-terminal")
          ?.textContent?.match(/TERMINAL_NATIVE_CONTINUED/g) ?? []
      ).length >= 2,
    undefined,
    { timeout: 90000 },
  );
  await page.keyboard.press("Escape");
  await page.keyboard.press("Tab");
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 900, height: 700 });
  await modal.getByRole("button", { name: "Close", exact: true }).click();
  await modal.waitFor({ state: "hidden", timeout: 20000 });
  console.log(
    "PASS native Agent terminal restored, answered, resized, accepted keys and closed",
  );
} catch (error) {
  // Synthetic conversation only; keep private native startup diagnostics outside version control.
  const page=browser.contexts()[0]?.pages()[0];
  if(page){const screen=await page.locator('.session-terminal').textContent().catch(()=>null);if(screen)fs.writeFileSync(process.argv[2]+'.screen',screen,{mode:0o600});}
  throw error;
} finally {
  await browser.close();
}
