# Wallpaper Web and licensed-library search contract

Date: 2026-08-31

This document fixes the implementation contract for the independent `Web`,
`Openverse`, and `Pexels` wallpaper sources. They do not change or silently
augment the existing X source.

## Product boundaries

- `Web` uses the fixed Grok Build Responses endpoint and the hosted
  `web_search` tool to discover real public source pages. It never asks the
  model to invent CDN image URLs.
- The Host fetches each discovered source page through the shared safe-HTTPS
  transport and extracts structured image metadata (`og:image`,
  `twitter:image`, and JSON-LD) with an HTML parser.
- `Openverse` searches the public Openverse Images API and retains the exact
  foreign landing page, creator, and Creative Commons license metadata.
- `Pexels` searches the official Pexels API when the user has configured an
  API key. The key is Host-only and follows the existing secrets backend.
- Every card retains a source-page link. Licensed-library cards additionally
  retain author and license links when the upstream response provides them.
- Search results are never described as copyright-free. The UI reports the
  upstream license and links users to the authoritative source page.

## Fixed endpoints and credentials

| Source | Endpoint | Credential |
|---|---|---|
| Web discovery | `https://cli-chat-proxy.grok.com/v1/responses` | Existing Grok Build OAuth access token, Host-only |
| Openverse | `https://api.openverse.org/v1/images/` | None for the public first release |
| Pexels | `https://api.pexels.com/v1/search` | User-supplied Pexels API key, Host-only |

The frontend cannot provide or override an endpoint, model, tool, bearer
token, redirect policy, or HTTP header. Responses requests use `grok-4.6`, low
reasoning effort, `store: false`, no redirect following, and request at most
six `web_search` calls per lane. The prompt asks the model to stop after one
call when it already has enough real source pages. Because the compatibility
endpoint has returned seven to nine completed calls despite that explicit
request limit, the Host records the overrun and accepts at most twelve observed
calls. A response above that separate hard ceiling is rejected. This keeps the
requested budget unchanged without discarding an already-paid, otherwise
valid result for a bounded provider-side overrun.

## Safe network pipeline

1. Accept only HTTPS URLs without userinfo.
2. Resolve and reject loopback, private, link-local, documentation, unspecified,
   and metadata-address destinations before every request.
3. Disable automatic redirects. Resolve each relative `Location`, re-run the
   URL and DNS checks, and allow at most three hops.
4. Bound HTML/API responses before parsing. Reject non-HTML source pages and
   non-JSON provider responses.
5. Resolve structured image URLs relative to the final page URL.
6. Fetch source documents and probe candidate media with fixed
   browser-compatible `Accept`, `Accept-Language`, and User-Agent values. Image
   probes may send only the source page's HTTPS origin as `Referer`; paths,
   query strings, cookies, authorization, and caller-provided headers are never
   forwarded.
7. Probe candidate media through the same per-hop checks. Require a real image
   signature, a compatible MIME type, a bounded size, and usable dimensions.
8. Re-run the same safety and media validation before saving a selected file.

No Cookie, browser storage, account token, authorization header, raw provider
response, or user query is written to logs or returned in diagnostics.

## Search, paging, cache, and cancellation

- Web search runs three complementary Responses lanes concurrently. The second
  lane explicitly combines the original topic with concise English search
  terms for non-English queries, while the third uses a distinct visual and
  editorial framing. Each lane aims for eight source pages and returns source
  metadata only. The initial search therefore sends at most three HTTP
  requests and asks for at most eighteen `web_search` calls; `Load more` uses
  one HTTP request and asks for at most six additional calls. The separate
  observed-response ceilings are thirty-six and twelve calls respectively.
  Responses discovery is bounded to 55 seconds.
  Source-page fetch and image validation then receive up to 30 seconds, with a
  20-second minimum after a slow Responses result so already-discovered pages
  are not discarded without validation. A complete lane remains bounded to
  about 75 seconds.
- A discovered source page may contribute at most two validated images. The
  Host probes at most six structured candidates from that page and stops its
  image probes after ten seconds. Media URLs remain globally deduplicated, so
  a useful gallery can retain a second distinct image from the same article
  without allowing one page's decorative assets to flood the result set.
- Global deduplication uses both the bounded content-prefix fingerprint and a
  normalized media-variant identity. Known resize, crop, quality, format, and
  CDN size-path variants collapse to the highest-resolution accepted card,
  while semantic image identifiers remain part of the identity. As a
  conservative fallback for recompressed copies, matching normalized titles
  from different source pages on the same site also collapse. Matching titles
  across sites do not collapse, and one source page may still contribute two
  distinct images.
- `Load more` runs one fresh lane with opaque existing source IDs so the model
  can avoid duplicates.
- After a successful remote search, the frontend keeps at most one result page
  ahead with a cancellable hidden prefetch. The visible gallery does not change
  until the user clicks `Load more`; that click consumes the buffered page and
  immediately starts the next prefetch when the Host reports more pages. This
  can spend one paging request even when the user never expands the gallery.
  Switching source, replacing the search, closing the modal, or cancelling
  invalidates the buffer and cancels its active request. Failed prefetches never
  clear existing cards and are not automatically retried.
- Openverse and Pexels use their documented page parameter. The Host owns the
  next-page state; the frontend cannot request arbitrary URLs.
- Search cache keys include source, normalized query, source-specific options,
  credential revision where applicable, and a contract version.
- Closing the modal, switching source, starting a replacement search, or using
  Cancel stops outstanding Responses, page-fetch, probe, and provider work.
- Cache entries contain only safe gallery DTOs and paging metadata. They never
  contain credentials, headers, or raw responses.

## Provenance DTO

All remote-source cards may carry these optional fields:

- `sourceUrl`, `sourceName`
- `authorName`, `authorUrl`
- `license`, `licenseUrl`

`Web` requires `sourceUrl`. `Openverse` and `Pexels` require `sourceUrl`,
`sourceName`, `authorName`, and license text; provider omissions are treated as
invalid rather than filled with guessed values.

## Explicit non-goals

- No Google Images or Bing Images result-page scraping.
- No login-wall bypass, CAPTCHA automation, browser Cookie export, or token
  export from the Grok Saved WebView.
- No invented source URLs, author names, license labels, dimensions, or counts.
- No automatic wallpaper application or rotation without a user selection.

## Implemented UX and verification snapshot

The first implementation keeps all seven sources in one dialog without mixing
their contracts:

- A labeled, horizontally scrollable source strip sits above one continuous
  full-width workspace; it is never collapsed into seven unlabeled icons.
- Controls, progress, route details, and errors appear only when relevant.
  Persistent source-description paragraphs and footer instructions are not
  rendered. Provenance remains attached to each result card.
- Existing cards stay interactive during foreground load-more work and hidden
  next-page prefetch. The load-more control reports and disables only its own
  operation.
- The local library shows all media in static-first order. Kind filters appear
  only for a mixed image/video collection, so an empty library has no
  zero-count filter row.

Focused frontend verification covers the remote controller, Web load-more card
interaction, the real Lightbox integration, Pexels key handling, gallery
rendering, and responsive layout guards. Two live Responses smoke runs exercised
the three-lane Web route. `薄雾森林风景摄影` completed in 55.0 seconds with seven
rendered images; two prefetched load-more batches then expanded the gallery from
7 to 10 and from 10 to 12 in about 0.2 seconds each. A third load-more operation
waited for its in-flight prefetch while existing cards remained clickable, and
the eventual prefetch timeout preserved all 12 images. `极光雪山湖泊 4K 壁纸`
also completed in 55.0 seconds, returning 11 candidates and retaining nine that
decoded successfully. These runs validate the `3 x 8` lane shape and the bounded
7-12 reported-tool-call compatibility window; they are smoke evidence rather
than a broad latency benchmark.
