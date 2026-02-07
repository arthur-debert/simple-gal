import { test, expect } from "@playwright/test";

// Minimal CSS subset for image page layout testing.
// Mirrors the actual generated CSS.
const CSS = `
  *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

  :root {
    --header-height: 3rem;
    --font-size-small: 12px;
    --frame-width-x: clamp(1rem, 3vw, 2.5rem);
    --frame-width-y: clamp(2rem, 6vw, 5rem);
    --color-bg: #ffffff;
    --color-text-muted: #666666;
    --color-border: #e0e0e0;
  }

  body { margin: 0; background: var(--color-bg); }

  .site-header {
    position: fixed; top: 0; left: 0; right: 0;
    height: var(--header-height);
    display: flex; align-items: center;
    background: var(--color-bg);
    border-bottom: 1px solid var(--color-border);
    z-index: 100;
  }

  main { margin-top: var(--header-height); }

  body.image-view { overflow: hidden; }

  body.image-view main {
    display: flex; align-items: center; justify-content: center;
    padding: var(--frame-width-y) var(--frame-width-x);
    height: calc(100dvh - var(--header-height));
  }

  .image-page {
    width: 100%; height: 100%;
    display: flex; align-items: center; justify-content: center;
    container-type: size;
  }

  .image-frame {
    aspect-ratio: var(--aspect-ratio);
    width: min(100%, calc(100cqh * var(--aspect-ratio)));
    height: min(100%, calc(100cqw / var(--aspect-ratio)));
  }

  .image-frame .placeholder {
    width: 100%; height: 100%; background: #ccc;
  }

  /* Short caption */
  body.has-caption { --caption-space: 2.5rem; }

  body.has-caption .image-page { flex-direction: column; }

  body.has-caption .image-frame {
    width: min(100%, calc((100cqh - var(--caption-space)) * var(--aspect-ratio)));
    height: min(calc(100cqh - var(--caption-space)), calc(100cqw / var(--aspect-ratio)));
  }

  .image-caption {
    width: min(100%, calc((100cqh - var(--caption-space)) * var(--aspect-ratio)));
    color: var(--color-text-muted);
    font-size: var(--font-size-small);
    padding-top: 0.75rem;
    line-height: 1.5;
    text-align: left;
  }

  /* Long description */
  body.has-description {
    overflow-y: auto;
    --description-peek: 5rem;
  }

  body.has-description main {
    height: auto;
    flex-direction: column;
    justify-content: flex-start;
    padding-bottom: 0;
  }

  body.has-description .image-page {
    height: calc(100dvh - var(--header-height) - var(--frame-width-y) - var(--description-peek));
    flex-shrink: 0;
  }

  .image-description {
    width: min(100%, calc((100dvh - var(--header-height) - var(--frame-width-y) - var(--description-peek)) * var(--aspect-ratio)));
    padding: 1.5rem 1rem 3rem;
    color: var(--color-text-muted);
    font-size: var(--font-size-small);
    line-height: 1.6;
  }

  .image-description p { white-space: pre-line; }
`;

/** Build a bare image page (no description). */
function bareImagePage(aspectRatio: number): string {
  return `<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><style>${CSS}</style></head>
<body class="image-view">
  <header class="site-header"></header>
  <main style="--aspect-ratio: ${aspectRatio};">
    <div class="image-page">
      <figure class="image-frame">
        <div class="placeholder"></div>
      </figure>
    </div>
  </main>
</body>
</html>`;
}

/** Build an image page with a short caption (inside .image-page). */
function captionImagePage(aspectRatio: number, caption: string): string {
  return `<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><style>${CSS}</style></head>
<body class="image-view has-caption">
  <header class="site-header"></header>
  <main style="--aspect-ratio: ${aspectRatio};">
    <div class="image-page">
      <figure class="image-frame">
        <div class="placeholder"></div>
      </figure>
      <p class="image-caption">${caption}</p>
    </div>
  </main>
</body>
</html>`;
}

/** Build an image page with a long description (outside .image-page, inside main). */
function descriptionImagePage(
  aspectRatio: number,
  description: string,
): string {
  return `<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><style>${CSS}</style></head>
<body class="image-view has-description">
  <header class="site-header"></header>
  <main style="--aspect-ratio: ${aspectRatio};">
    <div class="image-page">
      <figure class="image-frame">
        <div class="placeholder"></div>
      </figure>
    </div>
    <div class="image-description">
      <p>${description}</p>
    </div>
  </main>
</body>
</html>`;
}

// ============================================================================
// No description
// ============================================================================

test("no description: image frame fills available space", async ({ page }) => {
  await page.setContent(bareImagePage(1.5));
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
  await page.setContent(captionImagePage(1.5, "A beautiful sunrise"));

  const frameBox = await page.locator(".image-frame").boundingBox();
  const captionBox = await page.locator(".image-caption").boundingBox();

  expect(frameBox).toBeTruthy();
  expect(captionBox).toBeTruthy();
  expect(captionBox!.width).toBeCloseTo(frameBox!.width, 0);
});

test("short caption: caption is below the frame", async ({ page }) => {
  await page.setContent(captionImagePage(1.5, "A beautiful sunrise"));

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
  await page.setContent(captionImagePage(0.667, "Short caption"));

  const frameBox = await page.locator(".image-frame").boundingBox();
  const captionBox = await page.locator(".image-caption").boundingBox();

  expect(frameBox).toBeTruthy();
  expect(captionBox).toBeTruthy();
  expect(captionBox!.width).toBeLessThanOrEqual(frameBox!.width + 1);
});

test("short caption: centered horizontally with frame", async ({ page }) => {
  await page.setContent(captionImagePage(0.667, "Centered text"));

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
  const text = "Word ".repeat(80);
  await page.setContent(descriptionImagePage(1.5, text));

  const frameBox = await page.locator(".image-frame").boundingBox();
  const descBox = await page.locator(".image-description").boundingBox();

  expect(frameBox).toBeTruthy();
  expect(descBox).toBeTruthy();
  expect(descBox!.width).toBeCloseTo(frameBox!.width, 0);
});

test("long description: does not exceed frame width (portrait)", async ({
  page,
}) => {
  const text = "Word ".repeat(100);
  await page.setContent(descriptionImagePage(0.5, text));

  const frameBox = await page.locator(".image-frame").boundingBox();
  const descBox = await page.locator(".image-description").boundingBox();

  expect(frameBox).toBeTruthy();
  expect(descBox).toBeTruthy();
  expect(descBox!.width).toBeLessThanOrEqual(frameBox!.width + 1);
});

test("long description: peeks at viewport bottom", async ({ page }) => {
  const lines = Array.from({ length: 20 }, (_, i) => `Line ${i + 1}`).join(
    "\n",
  );
  await page.setContent(descriptionImagePage(1.5, lines));

  const descBox = await page.locator(".image-description").boundingBox();
  const viewport = page.viewportSize()!;

  expect(descBox).toBeTruthy();

  // Description should start within the viewport (peek visible)
  expect(descBox!.y).toBeLessThan(viewport.height);

  // Description should extend below the viewport (more content to scroll to)
  expect(descBox!.y + descBox!.height).toBeGreaterThan(viewport.height);
});

test("long description: page is scrollable", async ({ page }) => {
  const lines = Array.from({ length: 20 }, (_, i) => `Line ${i + 1}`).join(
    "\n",
  );
  await page.setContent(descriptionImagePage(1.5, lines));

  const scrollable = await page.evaluate(
    () => document.documentElement.scrollHeight > window.innerHeight,
  );
  expect(scrollable).toBe(true);
});

test("long description: is below the image area", async ({ page }) => {
  const text = "A".repeat(300);
  await page.setContent(descriptionImagePage(1.5, text));

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
  await page.setContent(captionImagePage(1.5, "Mobile caption"));

  const frameBox = await page.locator(".image-frame").boundingBox();
  const captionBox = await page.locator(".image-caption").boundingBox();

  expect(frameBox).toBeTruthy();
  expect(captionBox).toBeTruthy();
  expect(captionBox!.width).toBeCloseTo(frameBox!.width, 0);
});

test("description width matches frame at wide viewport", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const text = "Word ".repeat(80);
  await page.setContent(descriptionImagePage(0.75, text));

  const frameBox = await page.locator(".image-frame").boundingBox();
  const descBox = await page.locator(".image-description").boundingBox();

  expect(frameBox).toBeTruthy();
  expect(descBox).toBeTruthy();
  expect(descBox!.width).toBeCloseTo(frameBox!.width, 0);
});
