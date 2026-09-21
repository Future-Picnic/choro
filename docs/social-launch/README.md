# Choro social launch

Display name **Choro**, preferred handle **chorodev**, website **https://choro.dev**. Public brand-profile creation was explicitly authorized on 2026-09-17. Launch posts remain drafts and future YouTube videos default to Private.

| Platform | Actual channel | Setup state |
|---|---|---|
| YouTube | [@chorodev](https://www.youtube.com/@chorodev) | Created; profile saved, 10 private playlists, Private upload default. New generated cover saved. |
| Facebook | [Choro Page](https://www.facebook.com/profile.php?id=61594239040077) | Created; bio, website, category and avatar saved. New generated cover saved. Saving the available `chorodev` username requires the owner's Facebook password confirmation. |
| Reddit | [r/chorodev](https://www.reddit.com/r/chorodev/) | Created Public; generated banners, avatar, description, website, six rules and six post flairs saved. |
| X | [@Chorodev](https://x.com/Chorodev) | Connected by owner; name, avatar, generated header, bio and website saved. |
| TikTok | [Choro](https://www.tiktok.com/@user5401420888198) | Connected by owner; name, avatar and bio saved. `chorodev` and `choro.dev` unavailable; alternative username pending. Website appears as text in bio. |
| Instagram | Not started | Account, handle and profile setup remain. |
| LinkedIn | Paused by owner | Copy and artwork prepared; no Page created. Futurepicnic parent-page structure to settle when resumed. |

The earlier [r/usechoro](https://www.reddit.com/r/usechoro/) remains Private. No separate Reddit user account was created.

## Review materials

- [Generated cover review sheet](imagegen-v2/review-sheet.png) and [generation prompts and originals](imagegen-v2/README.md)
- [Page copy, Reddit rules, and unpublished launch-post drafts](page-copy.md)
- [YouTube playlist plan](youtube-playlists.md) and [61 recording briefs](youtube-library.json)
- [Platform setup record and screenshots](platform-setup.md)
- [Machine-readable setup status](setup-status.json)

## Current artwork

| Asset | File |
|---|---|
| Unchanged production avatar | [1024 × 1024 PNG](assets/choro-profile-1024.png) |
| YouTube cover | [2560 × 1440 PNG](imagegen-v2/youtube-cover.png) |
| Facebook cover | [1640 × 924 PNG](imagegen-v2/facebook-cover.png) |
| LinkedIn cover | [4200 × 700 PNG](imagegen-v2/linkedin-cover.png) |
| Reddit desktop cover | [4000 × 192 PNG](imagegen-v2/reddit-cover.png) |
| Reddit mobile cover | [PNG](imagegen-v2/reddit-mobile.png) |
| Existing YouTube thumbnail template | [1280 × 720 PNG](assets/youtube-thumbnail-template-1280x720.png) |

The covers were generated with the built-in ImageGen tool, then proportionally formatted for platform layouts. The existing app icon is unchanged. Original generations and previous cover assets are retained.

`python3 docs/social-launch/source/build_assets.py` rebuilds the earlier vector-based assets and library package; it does not regenerate ImageGen covers. Current publication state is recorded separately in `platform-setup.md` and `setup-status.json`.
