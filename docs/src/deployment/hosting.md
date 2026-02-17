# Hosting

Simple Gal produces a self-contained static site in `dist/`. There is no server-side processing, no database, no runtime. Any service that can serve files over HTTP will work.

## GitHub Pages

The easiest option if your source is already on GitHub. See [GitHub Actions](github-actions.md) for a complete workflow that builds and deploys automatically.

For manual deployment, push the contents of `dist/` to a `gh-pages` branch:

```bash
# Build locally
simple-gal build

# Deploy to gh-pages branch
npx gh-pages -d dist
```

Then set the Pages source to the `gh-pages` branch in your repository settings.

## Netlify

Connect your GitHub repository and configure the build:

| Setting | Value |
|---------|-------|
| Build command | *(leave blank -- use a GitHub Action to build, or install simple-gal in a build plugin)* |
| Publish directory | `dist` |

The simplest approach is to build with GitHub Actions and deploy the `dist/` directory. Alternatively, commit the built `dist/` directory and point Netlify at it directly.

For drag-and-drop deployment without any repository, use the Netlify CLI:

```bash
simple-gal build
npx netlify-cli deploy --dir dist --prod
```

## Vercel

Similar to Netlify. Connect your repository and set the output directory to `dist`.

For manual deployment:

```bash
simple-gal build
npx vercel --prod dist
```

Vercel will serve the static files without any framework configuration.

## Amazon S3 + CloudFront

For AWS hosting:

```bash
# Build the site
simple-gal build

# Sync to S3
aws s3 sync dist/ s3://your-bucket-name --delete

# Invalidate CloudFront cache (if using a distribution)
aws cloudfront create-invalidation \
  --distribution-id YOUR_DIST_ID \
  --paths "/*"
```

Configure the S3 bucket for static website hosting and point CloudFront at it for HTTPS and caching.

## Nginx

Serve `dist/` directly. A minimal configuration:

```nginx
server {
    listen 80;
    server_name photos.example.com;
    root /var/www/photos;

    location / {
        try_files $uri $uri/index.html =404;
    }
}
```

Copy the built site to the server:

```bash
simple-gal build
rsync -avz dist/ user@server:/var/www/photos/
```

## Apache

Enable `mod_dir` (usually on by default) and point the document root at the output directory:

```apache
<VirtualHost *:80>
    ServerName photos.example.com
    DocumentRoot /var/www/photos

    <Directory /var/www/photos>
        Options -Indexes
        AllowOverride None
        Require all granted
    </Directory>
</VirtualHost>
```

## General advice

- **HTTPS is required for PWA features.** The service worker will not register over plain HTTP (except on `localhost`). Most hosting services provide free SSL certificates.
- **No special server rules needed.** Simple Gal generates clean `index.html` files in each directory, so standard static file serving works without URL rewriting.
- **Cache headers are optional.** The service worker handles caching on the client side. If you want to set server-side cache headers, long cache times on image files (`/images/**`) are safe since filenames include content hashes.
