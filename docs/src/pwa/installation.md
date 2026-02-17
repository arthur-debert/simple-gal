# Installing as an App

Your visitors can install your portfolio as an app on their device. The process is slightly different on each platform, but in every case it takes a few seconds and requires no app store.

## iOS (Safari)

Safari does not show an automatic install prompt. Visitors use the Share menu:

1. Open your portfolio in Safari.
2. Tap the **Share** button (the square with an upward arrow).
3. Scroll down and tap **Add to Home Screen**.
4. Optionally edit the name, then tap **Add**.

Your portfolio now appears on the home screen with its icon. Tapping it opens the site in standalone mode -- full screen, no Safari UI.

> **Note:** On iOS, the PWA must be installed from Safari. Other browsers (Chrome, Firefox) on iOS do not support Add to Home Screen.

## Android (Chrome)

Chrome shows an install banner automatically when it detects a valid PWA. Visitors can also install manually:

1. Open your portfolio in Chrome.
2. Tap the **three-dot menu** in the top-right corner.
3. Tap **Add to Home Screen** (or **Install app** on newer versions).
4. Confirm by tapping **Add**.

The portfolio appears in the app drawer and on the home screen, and opens without browser chrome.

## Desktop (Chrome, Edge)

Desktop browsers also support PWA installation:

**Chrome:**
1. Visit your portfolio.
2. Click the **install icon** in the address bar (a monitor with a down arrow), or open the three-dot menu and select **Install**.

**Edge:**
1. Visit your portfolio.
2. Click the **App available** icon in the address bar, or open the three-dot menu and select **Apps > Install this site as an app**.

Once installed, the portfolio opens in its own window without browser tabs or address bar.

## What visitors see after installation

Regardless of platform, the installed app:

- Uses the name from your `site_title` configuration.
- Shows your custom icon if you placed one in `assets/` (see [Customizing](customizing.md)), or a default icon otherwise.
- Opens in standalone display mode -- the full screen is your portfolio, with no browser UI.
- Loads cached content instantly on launch.
