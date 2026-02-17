# GitHub Actions

The [`simple-gal-action`](https://github.com/arthur-debert/simple-gal-action) GitHub Action builds your site in CI without installing anything locally. It downloads the Simple Gal binary, runs the build, and produces a `dist/` directory ready for deployment.

## Full workflow: build and deploy to GitHub Pages

Create `.github/workflows/deploy.yml` in your repository:

```yaml
name: Deploy to GitHub Pages

on:
  push:
    branches: [main]

  # Allow manual trigger from the Actions tab
  workflow_dispatch:

# Required permissions for GitHub Pages deployment
permissions:
  contents: read
  pages: write
  id-token: write

# Prevent concurrent deployments
concurrency:
  group: pages
  cancel-in-progress: false

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Build site
        uses: arthur-debert/simple-gal-action@v1

      - name: Upload Pages artifact
        uses: actions/upload-pages-artifact@v3
        with:
          path: dist

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - name: Deploy to GitHub Pages
        id: deployment
        uses: actions/deploy-pages@v4
```

This workflow:

1. Triggers on every push to `main` (and can be triggered manually).
2. Checks out your repository.
3. Runs `simple-gal build` via the action, producing `dist/`.
4. Uploads `dist/` as a GitHub Pages artifact.
5. Deploys to GitHub Pages.

## GitHub Pages setup

Before the workflow can deploy, enable GitHub Pages in your repository settings:

1. Go to **Settings > Pages**.
2. Under **Source**, select **GitHub Actions**.

That is all. The workflow handles the rest.

## Custom domain

To use a custom domain (e.g., `photos.example.com`):

1. In your repository settings under **Pages > Custom domain**, enter your domain.
2. Add a `CNAME` file to the root of your content directory containing just the domain name:

```text
photos.example.com
```

3. Configure DNS with your domain registrar -- a CNAME record pointing to `<username>.github.io`.

GitHub will provision an SSL certificate automatically.

## Custom source directory

If your content is not in the default `content/` directory, pass options to the action:

```yaml
- name: Build site
  uses: arthur-debert/simple-gal-action@v1
  with:
    source: photos
    output: dist
```

## Caching

Image processing is the slowest step in a build. The action output (`dist/`) includes processed images, so subsequent builds that use a cache of the output directory can skip reprocessing unchanged images. GitHub's `actions/cache` can help here if build times become a concern.
