# slotadvance.com

The slot.advance website: one static page, served by Cloudflare (Workers with static assets).

| File | What it is |
|---|---|
| `public/index.html` | The page. |
| `public/logo.png` | The striped slot.advance logo, cut from `artwork/bootlogo/slot-advance-logo-source.png` on a transparent background. |
| `public/og-0.5.0.jpg` | Link-preview image (Open Graph), 1200 x 630: a crop of the current README hero, `.github/resources/slot-advance-hero-standby.webp` (full width, rows 105 to 945, scaled from 1600 px). Replaces `og.jpg`, which still carried the older "72% Less Standby Current." line. A new image gets a new file name, so link previews don't keep a cached old one. |
| `public/favicon.svg` | Browser-tab icon: the six logo stripes. |
| `public/slot-advance-0.5.0-r3.mp4` | The 0.5.0 teaser, played on the page: H.264 High 1080p at 30 fps, 61.0 s, about 10.2 MB (Cloudflare's limit is 25 MiB per file), with fast start. r3 replaces r2's picture with the cut that animates the lid opening and closing (same shots, captions and timings). Picture: CRF 18 encode of the r3 master. Audio: the supplied master's soundtrack with the same single gain as r2 (-2.73 dB, no compression or limiting), measured -13.0 LUFS integrated and -2.0 dBFS peak, AAC-LC 320 kbps stereo. A new video gets a new file name, so browsers never show a cached old one. |
| `public/video-poster-0.5.0-r3.jpg` | The teaser's opening card at 5 s, shown before it plays. |
| `wrangler.jsonc` | Cloudflare settings: serve everything in `public/`. |

Cloudflare builds from this folder (root directory `website`). A change pushed to `main` goes live automatically once the repository is connected.
