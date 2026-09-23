# Wholething connection status — September 22, 2026

Workspace: **Choro**, https://app.wholething.media/launches.

**Verified result: four connected channels.** X, Facebook and Instagram were connected during this setup. YouTube was already listed as Choro and was left untouched; Wholething does not expose its exact channel ID in the channel card.

| Platform | Status | Remaining work |
|---|---|---|
| YouTube | Existing Choro connection listed | Expected @chorodev; exact underlying channel ID not exposed by Wholething |
| X @Chorodev | Connected | None for connection; publishing was not tested |
| Facebook Choro | Connected | Page ID `61594239040077`; publishing was not tested |
| Instagram @choro.dev | Connected via Facebook Business | Publishing was not tested |
| TikTok | Choro production draft saved; owner reports submitted for review; not connected | Await review response, address requirements, install credentials and authorize account |
| Reddit | Not connected | Reddit Data API / Responsible Builder approval required before app credentials can be issued |
| LinkedIn | On hold | Owner previously chose to wait until launch |

[Verified four-channel dashboard](previews/wholething-four-channels-20260922.png).

## Configuration completed

The official [Postiz provider guide](https://docs.postiz.com/self-host/providers/overview) explains that social providers require developer apps and server environment variables, even though unconfigured providers still appear in Add Channel. The [configuration reference](https://docs.postiz.com/self-host/configuration/reference) requires restarting the application after changes.

- Found the existing DigitalOcean `wholething-production` deployment at `/opt/wholething`, serving `https://app.wholething.media`.
- Created a dedicated **Wholething Postiz** X developer app, configured OAuth read/write, Native App and the exact `/integrations/social/x` callback. Backed up the previous environment, installed credentials securely in production and recreated only the app container. Confirmed successful restart and @Chorodev authorization. No credentials are stored in this repository.
- Corrected the existing Meta app **Ritmus Postiz Test**: saved App Domains `wholething.media` and `app.wholething.media`, and production callback paths `/integrations/social/facebook` and `/integrations/social/instagram`. An additional `/integrations/social/instagram-business` entry added during setup remains, but the live Instagram flow uses `/instagram`.
- Linked @choro.dev to the Choro Facebook Page. Meta automatically created a private **Choro Business portfolio**, containing only those two assets, with one person holding full control. Instagram inbox access remains disabled. No OnlinePianist assets were authorized.
- Completed Facebook and Instagram OAuth and independently verified all four channels in Wholething.

## TikTok remaining setup

Created developer access and a **Wholething Postiz** production app draft. A fresh inspection showed **no configured products** and **“No scopes yet”**: the earlier product/scope selections did not persist. Reapply the intended Login Kit, Content Posting API, Direct Post, callback and required scopes once the required policy URLs are available, and verify persistence. **TikTok credentials have not been installed in production.**

**Domain verification is now complete.** TikTok confirmed `wholething.media` as Verified. The exact public TXT record is:

- Hostinger domain: `wholething.media`; host: `@`; TTL: `3600`
- Value: `tiktok-developers-site-verification=ECBIJaF8qwNpWTTLf9OYYXYwHPDsYaNr`

The value matched individually on both Hostinger authoritative nameservers (`solar.dns-parking.com`, `lunar.dns-parking.com`), Google DNS, Cloudflare DNS and Hostinger's zone UI. No DNS change was necessary; the production verification retry succeeded.

The user reports policy drafts, footer links and product information prepared in the Wholething project, but the public `/privacy` and `/terms` URLs still return 404. Final policies need the actual business/contact details, contractual terms, retention/deletion procedures and vendor disclosures. TikTok currently reports the missing Terms and Privacy URLs as two form errors. Demo and submission remain pending. The user explicitly wants a production integration, not a Sandbox workaround. See the [Postiz TikTok guide](https://docs.postiz.com/self-host/providers/tiktok).

## Reddit remaining setup

Reddit's live legacy-app registration flow required Data API / Responsible Builder approval. No app credentials were issued and no request or support ticket was submitted. Any access request must accurately describe the intended use; a moderation use case must not be invented to obtain access. The intended community is **r/chorodev**, not the earlier private r/usechoro.

No posts, schedules, messages, follows, upgrades or LinkedIn changes were made during this integration task. Existing content and connections were preserved.

## Latest policy-page handoff and draft-save attempt

The slash-terminated URLs `https://wholething.media/privacy/` and `https://wholething.media/terms/` now return HTTP 200 without login. Both explicitly remain owner-review drafts, not effective policies; they must not be submitted as final. This supersedes the earlier 404 observation above.

Aside session `wZ9Yoy7Yb0gtJaRT` entered the policy URLs, Login Kit, Content Posting API/Direct Post, exact callback and six connector scopes. TikTok rejected Save because app icon, category, description, review explanation and a real end-to-end demo video were missing. These entries remain **unsaved in the open tab**, not persisted configuration. No review submission or production credentials installation occurred.

The Wholething project readiness record additionally identifies unresolved audience eligibility and creator-info/privacy/interaction/upload implementation gaps. Those findings require resolution before review; final policies alone are insufficient. The owner confirmed the app is for the internal team only, managing its own accounts; review eligibility for that use remains unresolved. Existing four social connections are unchanged.

## Choro branding update

The owner requested Choro branding with no public Wholething or Postiz naming. Aside filled the existing TikTok form with **Choro**, the approved 1024px icon, **Productivity**, and truthful description/review text explaining internal use. Save was attempted again: **one validation error remains, the required real end-to-end demo video**. All changes remain unsaved in the open tab; the header still displays the previously saved app name. The existing application, callback and policy URLs still expose `wholething.media`. No review was submitted, no credentials installed and no existing connections changed.

## Video upload, saved draft and owner submission

Uploaded the existing 96-second Choro product overview as `choro-product-overview.mp4`, with an explicit review note that it does not demonstrate TikTok Login Kit or publishing. TikTok accepted **Save**. Reload verified Choro branding, icon, category, description, review explanation, Login Kit, Content Posting API/Direct Post, six scopes, callback, policy URLs and video persisted. This supersedes earlier unsaved-form observations.

The owner subsequently reported submitting the app for review and chose to wait for TikTok feedback; submission has not been independently verified. No submission was made by the agent. The real integration demo and final policy content remain outstanding. A live connection attempt returned `client_key` undefined; production credentials remain uninstalled. Existing social connections are unchanged.
