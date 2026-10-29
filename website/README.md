# slotadvance.com

The slot.advance website: one static page, served by Cloudflare (Workers with static assets).

| File | What it is |
|---|---|
| `public/index.html` | The page. |
| `public/logo.png` | The striped slot.advance logo, cut from `artwork/bootlogo/slot-advance-logo-source.png` on a transparent background. |
| `public/og.jpg` | Link-preview image, cropped from the README hero. |
| `public/favicon.svg` | Browser-tab icon: the six logo stripes. |
| `public/slot-advance-0.5.0.mp4` | The 0.5.0 teaser, played on the page: H.264 High 1080p at 29.97 fps with AAC stereo audio, 61 s, about 9.0 MB (Cloudflare's limit is 25 MiB per file). Re-encoded from the supplied master at CRF 18 with fast start; the creative content is unchanged. A new video gets a new file name, so browsers never show a cached old one. |
| `public/video-poster-0.5.0.jpg` | The teaser's opening card at 5 s, shown before it plays. |
| `wrangler.jsonc` | Cloudflare settings: serve everything in `public/`. |

Cloudflare builds from this folder (root directory `website`). A change pushed to `main` goes live automatically once the repository is connected.
