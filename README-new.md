# Simple Gal: In It For The Long Run

**Simple Gal** is a generator for web image galleries, built on a single core principle: **software should last**.

<p align="center">
  <img src="static/preview.png" alt="Simple Gal Preview" width="600">
</p>

## The Manifesto

There are a million web image generators and platforms. We have all been burned by them.

*   **Platforms disappear.** They get acquired, shut down, or pivot to video/AI/bloatware you don't need.
*   **Complex tools break.** Self-hosted solutions often require databases, specific PHP versions, or Docker containers that become security liabilities or maintenance nightmares.
*   **Data gets locked in.** Custom formats, databases, and "cloud" storage make it hard to leave.

**Simple Gal is designed to work 30 years from now.**

If you save the binary today, in 2056 it will still generate your site, provided:
1.  **x86/ARM processors** still exist (or can be emulated).
2.  **Browsers** still support HTML and CSS.
3.  **ImageMagick** (or a compatible binary) exists.

That's it. No databases. No migrations. No "cloud".

## Core Philosophy

### 1. The Filesystem is the Source of Truth
Your data is just folders and files.
*   **Albums** are directories.
*   **Ordering** is done via filenames (`001-My-Image.jpg`).
*   **Metadata** lives in sidecar text files or standard IPTC headers.
*   **Moving data** in and out is as simple as copying files.

### 2. Photography First
Designed for photographers, not bloggers.
*   **Precise Control**: You control the aspect ratios, sharpening, and compression quality.
*   **Adaptive Images**: Generates multiple sizes to ensure your photos look great on 4K monitors and phones alike.
*   **Distraction Free**: Minimal, clean UI that gets out of the way.

### 3. Extreme Simplicity
*   **Input**: A folder of images.
*   **Output**: Static HTML/CSS files.
*   **Deployment**: Copy the `dist` folder to *any* web server (Nginx, Apache, GitHub Pages, Netlify, an S3 bucket).

## Getting Started

### Option A: GitHub Integration (Recommended)
1.  **Fork** this repository.
2.  **Clone** your fork.
3.  **Replace** the contents of `content/` with your own images.
4.  **Push** your changes.
    *   The included GitHub Actions will automatically build your site and publish it to GitHub Pages.

### Option B: Run Locally
1.  **Download** the latest binary for your OS from [Releases](#).
2.  **Install Dependencies**: You need `ImageMagick` (see [DEPENDENCIES.md](DEPENDENCIES.md)).
3.  **Run**:
    ```bash
    # Generate site from 'content' folder to 'dist' folder
    simple-gal build
    ```
4.  **Preview**:
    ```bash
    # Serve the 'dist' folder
    python3 -m http.server --directory dist
    ```

## How It Works

### Directory Structure

The structure of your `content` directory defines your site.

```
content/
├── config.toml                  # Optional configuration
├── 010-Travel/                  # Album (010 = sort order)
│   ├── info.txt                 # Album description
│   ├── 001-Paris.jpg            # Image
│   ├── 002-London.jpg
│   └── 003-Rome/                # Nested Album
├── 020-Personal/
│   └── ...
└── 090-About.md                 # Standalone Page
```

### Naming Convention
We use a simple `NNN-Name` convention to handle sorting and titling simultaneously.
*   **`001-`**: Determines the sort order.
*   **`Name`**: Becomes the title (dashes become spaces).
    *   `010-Summer-Trip` -> Title: "Summer Trip", Position: 10.
*   **Hidden Items**: Items without a number prefix are processed but hidden from the navigation menu (great for drafts).

### Configuration
A single `config.toml` file controls the generator. Defaults are sensible, but everything is tweakable.

```toml
[images]
sizes = [800, 1400, 2080] # Generate these widths
quality = 90              # High quality by default

[theme]
# Control the whitespace around your images
frame_x = { size = "3vw", min = "1rem", max = "2.5rem" }
```

## Tech Stack (The "Forever" Stack)

*   **Generator**: A single Rust binary. fast, safe, and portable.
*   **Image Processing**: ImageMagick. The industry standard.
*   **Frontend**: Pure HTML5 and CSS. No React, no Vue, no bundlers.
    *   < 100 lines of vanilla JavaScript for navigation.
    *   Dark/Light mode support via CSS variables.

## Future Proofing

In order for Simple Gal to stop working, one of the following must happen:
1.  **Unix-like systems** cease to exist.
2.  **Browsers** drop support for standard HTML/CSS.
3.  **JPEG/WebP** formats are deprecated.

We are betting against all three.

---
*Built for the long haul.*
