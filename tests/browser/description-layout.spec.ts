import { test, expect } from "@playwright/test";
import { loadGeneratedPage, pages } from "./helpers";

// ============================================================================
// No description
// ============================================================================

test("no description: image frame fills available space", async ({ page }) => {
  await loadGeneratedPage(page, pages.noDescription.landscape);
  const frame = page.locator(".image-frame");
  const box = await frame.boundingBox();
  expect(box).toBeTruthy();
  expect(box!.width).toBeGreaterThan(100);
  expect(box!.height).toBeGreaterThan(100);
});

// ============================================================================
// Short caption
// ============================================================================

test("short caption: caption width matches frame width", async ({ page }) => {
  await loadGeneratedPage(page, pages.withCaption.landscape);

  const frameBox = await page.locator(".image-frame").boundingBox();
  const captionBox = await page.locator(".image-caption").boundingBox();

  expect(frameBox).toBeTruthy();
  expect(captionBox).toBeTruthy();
  expect(captionBox!.width).toBeCloseTo(frameBox!.width, 0);
});

test("short caption: caption is below the frame", async ({ page }) => {
  await loadGeneratedPage(page, pages.withCaption.landscape);

  const frameBox = await page.locator(".image-frame").boundingBox();
  const captionBox = await page.locator(".image-caption").boundingBox();

  expect(frameBox).toBeTruthy();
  expect(captionBox).toBeTruthy();
  expect(captionBox!.y).toBeGreaterThanOrEqual(
    frameBox!.y + frameBox!.height - 1,
  );
});

test("short caption: does not exceed frame width (portrait)", async ({
  page,
}) => {
  await loadGeneratedPage(page, pages.withCaption.portrait);

  const frameBox = await page.locator(".image-frame").boundingBox();
  const captionBox = await page.locator(".image-caption").boundingBox();

  expect(frameBox).toBeTruthy();
  expect(captionBox).toBeTruthy();
  expect(captionBox!.width).toBeLessThanOrEqual(frameBox!.width + 1);
});

test("short caption: centered horizontally with frame", async ({ page }) => {
  await loadGeneratedPage(page, pages.withCaption.portrait);

  const frameBox = await page.locator(".image-frame").boundingBox();
  const captionBox = await page.locator(".image-caption").boundingBox();

  expect(frameBox).toBeTruthy();
  expect(captionBox).toBeTruthy();

  const frameCenterX = frameBox!.x + frameBox!.width / 2;
  const captionCenterX = captionBox!.x + captionBox!.width / 2;
  expect(captionCenterX).toBeCloseTo(frameCenterX, 0);
});

// ============================================================================
// Long description
// ============================================================================

test("long description: width matches frame width", async ({ page }) => {
  await loadGeneratedPage(page, pages.withDescription.landscape);

  const frameBox = await page.locator(".image-frame").boundingBox();
  const descBox = await page.locator(".image-description").boundingBox();

  expect(frameBox).toBeTruthy();
  expect(descBox).toBeTruthy();
  expect(descBox!.width).toBeCloseTo(frameBox!.width, 0);
});

test("long description: does not exceed frame width (portrait)", async ({
  page,
}) => {
  await loadGeneratedPage(page, pages.withDescription.portrait, {
    "--aspect-ratio": "0.5",
  });

  const frameBox = await page.locator(".image-frame").boundingBox();
  const descBox = await page.locator(".image-description").boundingBox();

  expect(frameBox).toBeTruthy();
  expect(descBox).toBeTruthy();
  expect(descBox!.width).toBeLessThanOrEqual(frameBox!.width + 1);
});

test("long description: peeks at viewport bottom", async ({ page }) => {
  await loadGeneratedPage(page, pages.withDescription.landscape);

  const descBox = await page.locator(".image-description").boundingBox();
  const viewport = page.viewportSize()!;

  expect(descBox).toBeTruthy();

  // Description should start within the viewport (peek visible)
  expect(descBox!.y).toBeLessThan(viewport.height);

  // Description should extend below the viewport (more content to scroll to)
  expect(descBox!.y + descBox!.height).toBeGreaterThan(viewport.height);
});

test("long description: page is scrollable", async ({ page }) => {
  await loadGeneratedPage(page, pages.withDescription.landscape);

  const scrollable = await page.evaluate(
    () => document.documentElement.scrollHeight > window.innerHeight,
  );
  expect(scrollable).toBe(true);
});

test("long description: is below the image area", async ({ page }) => {
  await loadGeneratedPage(page, pages.withDescription.landscape);

  const imagePageBox = await page.locator(".image-page").boundingBox();
  const descBox = await page.locator(".image-description").boundingBox();

  expect(imagePageBox).toBeTruthy();
  expect(descBox).toBeTruthy();

  // Description should start at or below the image page
  expect(descBox!.y).toBeGreaterThanOrEqual(
    imagePageBox!.y + imagePageBox!.height - 1,
  );
});

// ============================================================================
// Viewport variations
// ============================================================================

test("caption width matches frame at narrow viewport", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 667 });
  await loadGeneratedPage(page, pages.withCaption.landscape);

  const frameBox = await page.locator(".image-frame").boundingBox();
  const captionBox = await page.locator(".image-caption").boundingBox();

  expect(frameBox).toBeTruthy();
  expect(captionBox).toBeTruthy();
  expect(captionBox!.width).toBeCloseTo(frameBox!.width, 0);
});

test("description width matches frame at wide viewport", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loadGeneratedPage(page, pages.withDescription.portrait, {
    "--aspect-ratio": "0.75",
  });

  const frameBox = await page.locator(".image-frame").boundingBox();
  const descBox = await page.locator(".image-description").boundingBox();

  expect(frameBox).toBeTruthy();
  expect(descBox).toBeTruthy();
  expect(descBox!.width).toBeCloseTo(frameBox!.width, 0);
});
