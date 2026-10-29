# slotadvance.com

The slot.advance website: one static page, served by Cloudflare (Workers with static assets).

| File | What it is |
|---|---|
| `public/index.html` | The page. |
| `public/logo.png` | The striped slot.advance logo, cut from `artwork/bootlogo/slot-advance-logo-source.png` on a transparent background. |
| `public/og.jpg` | Link-preview image, cropped from the README hero. |
| `public/favicon.svg` | Browser-tab icon: the six logo stripes. |
| `public/slot-advance-0.5.0-r2.mp4` | The 0.5.0 teaser, played on the page: H.264 High 1080p at 29.97 fps, 61.2 s, about 10.5 MB (Cloudflare's limit is 25 MiB per file), with fast start. Picture: the CRF 18 re-encode of the supplied master, unchanged since the first 0.5.0 page. Audio (r2): the master's AAC, which peaked above full scale (+0.6 dBFS sample, +0.7 dBTP, -10.3 LUFS), lowered by a single measured gain of -2.73 dB (linear loudness normalisation to -13 LUFS, true peak -2.0 dBTP, no compression or limiting) and re-encoded as AAC-LC 320 kbps stereo. A new video gets a new file name, so browsers never show a cached old one. |
| `public/video-poster-0.5.0.jpg` | The teaser's opening card at 5 s, shown before it plays. |
| `wrangler.jsonc` | Cloudflare settings: serve everything in `public/`. |

Cloudflare builds from this folder (root directory `website`). A change pushed to `main` goes live automatically once the repository is connected.
