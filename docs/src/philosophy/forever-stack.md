# The Forever Stack

Simple Gal is designed to produce gallery sites that work 20 years from now. This page explains what that claim rests on, what could break it, and what we give up to make it credible.

## The claim

If you run `simple-gal build` today and put the output on a file server, someone should be able to open that site in a browser in 2045 and see your photographs exactly as you intended. No software updates required. No server migration. No database restoration. Just files served over HTTP.

## Why we believe it

The output of Simple Gal consists of:

- **HTML** -- the foundational document format of the web, backward-compatible since the 1990s. Simple Gal uses basic, well-established elements. No custom elements, no Web Components, no framework-specific markup.
- **CSS** -- inline styles and a single stylesheet using properties that have been stable for over a decade. No CSS-in-JS, no preprocessor, no build step.
- **AVIF images** -- the output image format. AVIF is based on the AV1 video codec and backed by the Alliance for Open Media (Google, Apple, Mozilla, Microsoft, Netflix, Amazon). It is an ISO standard (ISO/IEC 23000-22). Browser support is universal in modern browsers.
- **~30 lines of vanilla JavaScript** -- for keyboard navigation and swipe gestures only. Click-based navigation is pure HTML anchor tags and CSS. If JavaScript stopped running entirely, you could still browse every photo by clicking.

There are no dependencies in the output. No `node_modules`, no CDN links, no third-party scripts, no API calls. The site is self-contained: a directory of files that reference only each other.

## The tool itself

The generator is a single Rust binary with no runtime dependencies. It reads files from disk and writes files to disk. To use it, you need exactly one thing: the ability to run a compiled program.

Pre-built binaries are provided for major platforms. Even if the Rust toolchain disappeared tomorrow, the existing binaries would continue to work on any compatible OS. And since the input is just a directory of images and text files, you could always rebuild the output with a different tool if needed -- there is no lock-in to a proprietary data format.

## What could break it

We should be honest about the assumptions:

**Browsers drop AVIF support.** This is the most format-specific risk. However, AVIF is an ISO standard backed by every major browser vendor, and the web has never dropped support for an image format once it reached universal adoption (GIF, JPEG, PNG, and WebP are all still supported decades after introduction). We consider this extremely unlikely.

**HTML fundamentally changes.** The `<img>`, `<a>`, and `<div>` elements that Simple Gal relies on have been stable since the mid-1990s. Browsers maintain backward compatibility with content from that era. A change that broke basic HTML would break a significant fraction of the existing web. This is not a realistic concern.

**The file server goes away.** Simple Gal generates static files, but someone still needs to serve them. If your hosting provider shuts down, you need to move the files. The good news is that static file hosting is the simplest, cheapest, most widely available form of web hosting. Moving a directory of files to a new server is a solved problem.

**You lose the source files.** Simple Gal does not replace your backup strategy. The generated site contains processed (resized, compressed) images, not your originals. If you lose the content directory, you lose the ability to rebuild at different settings. Back up your source files.

## What we give up

The Forever Stack is a deliberate tradeoff: longevity and simplicity over features. Here is what Simple Gal does not do, and will not do:

- **No comments.** Commenting systems require a server, a database, and moderation. They are a maintenance liability with a finite lifespan.
- **No search.** Client-side search requires a JavaScript index that grows with the site. Server-side search requires infrastructure. Neither fits the model.
- **No infinite scroll.** Infinite scroll depends on JavaScript for loading and layout. Page-based navigation is pure HTML and works without scripting.
- **No analytics (built in).** Tracking visitors requires either JavaScript or server logs. Simple Gal does not inject tracking code. You can add your own via the `assets/head.html` injection point if you want it.
- **No image lazy-loading via JavaScript.** The site uses native browser `loading="lazy"` attributes, which are HTML-only and require no scripting.

Each of these omissions removes a dependency, a maintenance burden, or a failure mode. The site does less, but what it does, it will keep doing.

## The tradeoff in practice

If you need comments, search, e-commerce, or client proofing, Simple Gal is not the right tool. Use something designed for those features.

If you want a portfolio that shows your photographs in a clean, fast, controllable way -- and you want it to still work without intervention in 2035, 2040, 2045 -- then the constraints are the point. Every feature we didn't add is a thing that can't break.
