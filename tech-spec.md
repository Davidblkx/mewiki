# me-wiki — tech spec

This spec answers [requirements.md](requirements.md). It refers to requirements and constraints by ID (R01, C01) and
does not repeat them.

## Overview

One Rust binary built on `axum` serves everything: the pages, the editor, the dashboard and the API. Markdown is
rendered on the server, and the result is cached on disk. Pages are plain HTML. The browser only runs a little
JavaScript: the editor, Mermaid for diagrams, the offline sync, and the service worker. Every static file, including
the JavaScript, is embedded in the binary, so the only thing on disk is the data folder (C02).

**A01** The wiki holds up to about 1,000 pages. Moves read every page to rewrite links, and the tree is a full scan
of `pages/`. Both are simple because of this assumption, and both would need an index well beyond it.

## Data folder

```
/data                       MEWIKI_DATA, the only volume (C02)
  pages/                    source of truth (C03), and the tree itself (C05)
    index.md                the home page, served at /
    mushroom.md             /mushroom
    mushroom/               subpages of /mushroom
      chanterelle.md        /mushroom/chanterelle
    recipes/                a group with no page of its own
      risotto.md            /recipes/risotto
  uploads/                  uploaded assets (R10), flat
    risotto-01.jpg
  cache/                    cached assets (C03, C06): safe to delete at any time
    pages/mushroom.html
    pages/mushroom/chanterelle.html
  config/
    secret.key              32 random bytes, created on first run, mode 0600
    custom.css              the owner's CSS from the dashboard (R02)
```

**A page is a `.md` file. Its subpages live in a folder with the same name next to it.** A folder with no matching
`.md` is a group. It shows in the tree, and its URL shows a blank page: the layout and tree with an empty body.
Groups only appear when files are moved or deleted outside the app; the app itself never creates one. A group's title
comes from the folder name, the same way a page without a heading gets its title.

**Page names** are path segments that match `[a-z0-9][a-z0-9-]*`. That rules out `..`, separators and leading
underscores, so a request path can never escape `pages/`. It also rules out leading dots, so a page name can never
collide with a temporary file. Paths starting with `/_/` are reserved for the app. At the top level, `index` is
reserved for the home page.

**The title** is the page's first `# heading`. Without one, the name is used: `wild-garlic` becomes "Wild garlic".
That way no metadata is needed for the title.

**The home page** (`index.md`) can be edited and protected, but never renamed, moved or deleted. Top-level pages sit
next to it, not under it. Protecting it therefore protects only `/`.

### Page metadata

Metadata goes in a front-matter block at the top of the `.md` file, not in a separate `.json`:

```markdown
---
protected: true
---
# Chanterelle
```

For now, `protected` is the only key the app reads. **The parser fails closed:** when the key is there, any value
other than exactly `false` counts as protected, including `yes`, `True` or nothing. A typo can hide a page, but it
can't make one public. Without the key, a page is public.

Keys the app doesn't recognise are kept exactly as they are when it saves the page. The parser is about twenty
lines: a block between two `---` lines, one `key: value` per line. That avoids a YAML dependency, and the block is
still valid front matter for Obsidian, Hugo and GitHub.

The editor doesn't show the block. Protection is a checkbox, and the server writes the block on save.

*Rejected: a `.json` next to each `.md`, as in the first draft.* Renaming or moving a page would mean moving two
files and keeping them paired, and a page edited outside the app could lose its metadata. Front matter travels with
the page.

## Rendering and the cache

**Rendering.** `comrak` turns Markdown into HTML with these extensions on: tables, task lists, strikethrough and
autolinks (R01). Raw HTML in Markdown is escaped (`comrak`'s default). Code blocks go through a highlighter adapter:
blocks tagged `mermaid` come out as `<pre class="mermaid">` with the source escaped. Everything else goes to
`syntect` with class-based output, so the theme lives in CSS and custom CSS can change it (R02). Because highlighting
happens on the server, it also works offline.

**The cache** holds the rendered page body only: `cache/pages/<path>.html`. The layout around it is added on every
request. That layout includes the navigation tree, the stylesheets, and the edit buttons when the owner is logged in.
Adding the layout is a string template, so it costs almost nothing. It means a new page or a CSS change never
requires re-rendering other pages.

**A cached body is stale** when its `.md` has a newer modification time. In that case it is re-rendered on the next
request. Pages edited outside the app therefore update on their own. The dashboard's **Rebuild** button deletes
`cache/` and renders every page. That is needed only after an upgrade changes the renderer.

**The tree** is built at startup by walking `pages/`, and is kept in memory. Every change made through the app
rebuilds it. Files added or moved outside the app appear after **Rebuild** or a restart. The tree drives navigation
only. Protection is never decided from it (see Authentication).

**Page HTML is sent with `Cache-Control: no-store`.** That covers pages, groups, "not found", the editor and the
dashboard. The browser's own cache therefore never holds a page from a logged-in session that could be shown after
logout. The service worker isn't affected by this header, so offline copies come only from it.

*Rejected: caching complete pages, layout included, as in the first draft.* Every new page, rename or CSS change
would change the tree or the styles in every page, so the whole wiki would be re-rendered on almost every edit.

## Routes

| Route | Who | Does |
| --- | --- | --- |
| `GET /` and `GET /{*path}` | anyone | page, group (blank page), or "not found" (R14) |
| `GET /_/static/*` | anyone | embedded CSS, JavaScript, Mermaid, icons |
| `GET /_/custom.css` | anyone | `config/custom.css` |
| `GET /_/uploads/{name}` | anyone | an uploaded asset (see Uploads) |
| `GET /_/health` | anyone | `200` when the data folder can be read and written, otherwise `503` |
| `GET /sw.js`, `GET /manifest.webmanifest` | anyone | PWA. The worker must be at the root to control `/` |
| `GET /_/login`, `POST /_/login`, `POST /_/logout` | anyone | session |
| `GET /_/edit/{*path}`, `GET /_/new?parent=` | owner | editor |
| `GET /_/dashboard` | owner | dashboard |
| `POST /_/api/preview` | owner | Markdown in, HTML body out (R08) |
| `PUT /_/api/page/{*path}` | owner | create or save `{ markdown, protected }` |
| `DELETE /_/api/page/{*path}?subpages=true` | owner | delete a page or group, and its subpages |
| `POST /_/api/move` | owner | rename or move `{ from, to, make_public }` |
| `POST /_/api/uploads`, `DELETE /_/api/uploads/{name}` | owner | upload, delete |
| `PUT /_/api/custom-css`, `POST /_/api/rebuild` | owner | dashboard actions |
| `GET /_/api/pages` | owner | every page's URL and title, for sync (R04) and the link picker |

When a visitor opens an owner page, they are sent to `/_/login`. When a visitor calls an owner API route, the answer
is `401` (R11).

## Changing pages

All writes go through one async mutex, so a move can never run at the same time as a save. A single owner doesn't
need anything finer. A file is written to `.<name>.tmp` in the same folder and then renamed over the original, so a
crash can't leave half a page. The tree skips dotfiles, and startup deletes any leftover `.*.tmp` under `pages/`,
`uploads/` and `config/`. When two devices save the same page, the last save wins (out of scope).

**Every create and move needs an existing parent.** The parent is the home page (top level), a page or a group. When
it doesn't exist, the answer is `409`, and the parent has to be created first. That keeps the rule that the app never
creates a group.

**Create** starts from `/_/new?parent=`. The form has two fields, **Title** and **Name**. Name fills in from the
title as you type (`Wild Garlic` becomes `wild-garlic`), and you can still edit it. The title becomes the page's
first heading.

**Create and save** write the `.md` file, render it into the cache, and rebuild the tree.

**Rename and move are one operation**, because a rename is a move within the same folder. The steps:

1. Refuse the move if the target already exists, if the target is inside the page's own subpages, if the target's
   parent doesn't exist, or if the page is the home page.
2. Check protection. If the page is protected only because of a page above it, and the target isn't under a protected
   page, the move would make it and its subpages public (R13). The editor's confirmation lists those pages. The
   server answers `409` with the same list unless the request says `make_public: true`.
3. Move the `.md` file and its subpage folder.
4. Rewrite links in every page (R07). Any link whose path is the old path, or starts with it followed by `/`, gets
   the new path in its place. Fragments are kept (`#section`).
5. Remove the moved pages from the cache, and re-render the pages whose links changed.

**Links** are standard Markdown links with absolute wiki paths: `[Chanterelle](/mushroom/chanterelle)`. They render
without the app and stay readable in the raw file. To rewrite them, the app parses each page with `comrak` and edits
only the link destinations. That way, text in code blocks that happens to look like a link is left alone. Reading
every page on each move is fine within A01.

**Only absolute links survive a move.** Relative links such as `[x](chanterelle)` are left as they are and can break.
To keep you from typing paths by hand, the editor's **Link** button opens a page picker filled from `/_/api/pages`,
and it inserts an absolute link.

*Rejected: links by stable page ID, such as `[[id:4f2a]]`.* These never need rewriting, but every page then needs an
ID in its metadata, an index from ID to path, and a raw file that no longer reads as Markdown. Links in pages moved
outside the app break in both designs, so IDs buy nothing there. *Rejected: rewriting relative links on save or on a
move.* The first changes text you typed, and the second doubles the link-resolution code for a case the picker
avoids.

**Delete** removes the page's `.md` file, its subpage folder with everything in it, and their cached bodies (R06).
Before deleting, the editor asks for confirmation, listing every page that will go. The server enforces the same
rule. A delete of a page that has subpages is answered with `409` and the list of subpages, unless the request says
`?subpages=true`, so a script or a stale tab can't remove a branch by accident. A group is deleted the same way, and
the home page can't be deleted. Uploads stay (R10), and links to deleted pages are left as they are, leading to
"not found".

The cost: there is no page history (out of scope), so a confirmed delete of a branch can't be undone, except from a
backup of the data folder (C02).

*Rejected: deleting only the page and leaving its subpages behind as a group.* Subpages protected only because of
the deleted page would quietly become public (R13).

## Authentication

**At startup** the server reads `MEWIKI_PASSWORD`. If the variable is missing or empty, the server stops with an
error. Running without it would leave protected pages unreadable and editing impossible. The password is kept in
memory only and never written to disk (R12).

**Sessions are signed cookies**, so no session list is stored:

- `key = HMAC-SHA256(secret.key, password)`
- cookie `mewiki_session = <expiry>.<hex(HMAC-SHA256(key, expiry))>`, valid for 90 days from login, not extended by
  use
- `HttpOnly`, `SameSite=Strict`, `Path=/`, and `Secure` unless `MEWIKI_COOKIE_SECURE=false`

The password is part of the key, so changing `MEWIKI_PASSWORD` cancels every session, and so does deleting
`secret.key`. Logout clears the cookie in that browser only. A stolen cookie stays valid until it expires or the
password changes. That's the cost of storing no sessions.

*Rejected: a session table in memory.* Every restart of the container would log the owner out. *Rejected: the first
draft's cookie, which held a value derived from the password.* It never expired and was the same on every device.

**Login** hashes the input and the password with SHA-256 and compares the two hashes in constant time (`subtle`).
Comparing hashes means the time taken doesn't reveal the password's length. Logins are handled one at a time, and a
failure waits one second before answering. That caps guessing at about 3,600 attempts an hour. It never locks the
owner out, which a failure counter would let an attacker do on purpose. The password still needs to be long. The
cost: while someone floods the login with guesses, your own login waits in the same queue.

**Cross-site requests** are blocked by `SameSite=Strict` alone. Current browsers don't send the cookie on any request
started by another site, so another site can't make your browser save, move or delete pages.

*Rejected: also checking that the `Origin` header matches the host.* Behind a reverse proxy, the host the app sees
can differ from the public address, so the check would reject your own saves unless the public URL were configured.

**Protection (R13, R14).** A page is protected when its own front matter, or the front matter of any page above it,
says it is (see Page metadata). Each request reads those front-matter blocks from disk: the page plus one small read
per level above it. The in-memory tree is never used for this, so a page protected by editing the file outside the
app is hidden right away.

A visitor who requests a protected page gets the same `404` and the same body as for a page that doesn't exist. The
tree still lists the page, with a lock icon for everyone. That icon comes from the tree, so it can lag behind edits
made outside the app until **Rebuild**. Groups have no file, so they can't be protected.

## Offline (R04)

The service worker uses three caches:

- **static**: CSS, JavaScript, Mermaid and icons, named after the app version, cache-first. A new version replaces the
  old cache.
- **pages**: page HTML and `/_/custom.css`, network-first. A response that loads is saved; when there's no network,
  the saved copy is used. A page with no saved copy shows a "not available offline" page.
- Everything else, meaning `/_/edit`, `/_/dashboard`, `/_/api`, `/_/login`, `/_/health` and `/_/uploads`, always goes
  to the network.

**A `404` is never saved, and it removes any saved copy of that URL.** A visitor's device therefore never holds a
protected page, and a page you delete or protect disappears from a device the next time that device opens it online.
Offline, saved copies stay until then.

**Sync** is a dashboard button. It fetches `/_/api/pages`, then requests each URL and stores it in the pages cache,
showing progress. The script uses the Cache API from the page itself, so no messages to the service worker are
needed. Logging out leaves the cache as it is.

Saved pages keep the tree and buttons from when they were saved, so offline navigation can be slightly out of date.

**Service workers need HTTPS**, except on `localhost`. This is a deployment requirement and not optional: without
HTTPS, R04 doesn't work.

## Uploads

- Files are saved in `uploads/` under a cleaned-up version of their name. A name already taken gets a suffix:
  `photo.jpg`, then `photo-1.jpg`. The editor's upload button inserts the Markdown link at the cursor.
- The upload route accepts at most 2 MB, enforced by `axum`'s body limit (R10).
- **Serving (the HTML and SVG risk).** Every upload response sets `X-Content-Type-Options: nosniff` and
  `Content-Security-Policy: sandbox`. PNG, JPEG, GIF, WebP, PDF and plain text are shown in the browser with their
  real type. Every other file, SVG and HTML included, is sent as `application/octet-stream` with
  `Content-Disposition: attachment`. A file opened directly therefore runs in a sandboxed, opaque origin, or is just
  downloaded. Either way it can't act as the logged-in owner.
- The dashboard lists uploads with their size and marks the ones no page links to. Deleted pages leave uploads
  behind (R10), so this is how the owner finds them.
- **Deleting an upload doesn't check whether any page uses it.** Pages that still link to it then show a broken image
  or link. The "unused" marker is the guide.
- Uploads are never protected and never stored offline (R04, R13).

## Configuration and deployment

| Variable | Default | |
| --- | --- | --- |
| `MEWIKI_PASSWORD` | none, required | the only password (R12) |
| `MEWIKI_DATA` | `/data` | the data folder (C02) |
| `MEWIKI_ADDR` | `0.0.0.0:8080` | listen address |
| `MEWIKI_COOKIE_SECURE` | `true` | set `false` only for plain-HTTP testing |
| `PUID`, `PGID` | `1000`, `1000` | the user and group the server runs as |

The app speaks plain HTTP, and a reverse proxy such as Caddy or Traefik adds HTTPS. Missing folders under `/data` are
created at startup.

**The Docker image** has two stages. The build stage compiles a static `musl` binary, and the final image holds only
that binary, with `/data` as its volume (C04).

**The user it runs as.** When the binary starts as root, it switches to `PUID`/`PGID` before it touches `/data` or
opens the port. The switch uses `setgid` and `setuid` from the `nix` crate, so no shell or entrypoint script is
needed in the image. When the container is already started as a non-root user, for example with `--user`, the binary
skips the switch. The README tells you to `chown` the host folder to the same IDs, and that the default `1000:1000`
is usually the first user on a Linux host.

**The health check** is the binary itself, `mewiki health`. The Dockerfile's `HEALTHCHECK` runs it. It sends a
`GET /_/health` over a plain TCP socket to `MEWIKI_ADDR` and exits `0` on `200`. The image has no `curl`, and no
HTTP client crate is needed for one request.

## Code layout

```
src/
  main.rs        config, startup (user switch, temp-file cleanup), router, the `health` subcommand
  store.rs       the data folder: path checks, safe writes, the tree, the write lock
  page.rs        front matter, titles, protection lookup, link rewriting
  render.rs      comrak, the syntect and mermaid adapter, the cache
  auth.rs        password, session cookies, the owner-only layer
  routes/        read.rs, edit.rs, api.rs, uploads.rs, dashboard.rs
templates/       askama templates: layout, page, group, editor, dashboard, login, not found
assets/          css, editor.js, sync.js, sw.js, vendored mermaid, icons, all embedded with rust-embed
```

**Crates:** `axum`, `tokio`, `tower-http`, `comrak` with its `syntect` feature, `askama`, `serde`, `serde_json`,
`hmac`, `sha2`, `subtle`, `rand`, `rust-embed`, `nix`, `tracing`. Templates use `askama` so the layout stays in HTML
files that are checked when the code compiles. *Rejected: `maud`, which writes HTML as Rust macros.* Changing the look
would mean editing Rust code.

Mermaid is a large file, so a page loads it only if its body contains `class="mermaid"`.

## Testing

- **Unit tests:**
  - page-name checks, including traversal attempts
  - front-matter parsing and saving: unknown keys are kept, and `yes`, `True` or an empty value count as protected
  - working out protection from parent pages
  - link rewriting: fragments, subpages, and text in code blocks that only looks like a link
  - signing and checking session cookies, including expiry and a changed password
- **Integration tests** run the full router against a temporary data folder, using `tower`'s `oneshot`:
  - a visitor gets the same `404` for a protected page and a missing one
  - a page protected by editing its file on disk is hidden on the next request
  - owner routes refuse anyone without a session
  - a move that rewrites links
  - moves that are refused: into the page's own subpages, under a missing parent, and the home page
  - a move that would make pages public is refused without `make_public: true`
  - a delete with subpages is refused without `?subpages=true`, and removes the whole branch with it
  - the home page can't be deleted
  - the upload size limit, and the headers on an uploaded SVG
  - `Cache-Control: no-store` on page HTML
  - `/_/health` answers `503` when the data folder is read-only
  - temporary files left in the data folder are removed at startup
- **Service worker:** tested by hand in a browser for the first version. The cases: visit a page and then go offline;
  sync; an uncached page while offline; a `404` removing a saved copy.

## Build order

Each step runs and is tested before the next one starts.

1. **Reading.** The data folder, the tree, rendering and the cache, the layout, and page and group routes. Content is
   written by hand into `pages/`.
2. **Login and editor.** Sessions, protection, the owner-only layer, the editor with preview and the link picker,
   create and save, and the dashboard's custom CSS and Rebuild.
3. **Move and delete,** with link rewriting and both confirmations.
4. **Uploads,** with serving rules and the dashboard list.
5. **Offline.** Manifest, service worker and Sync.
6. **Docker.** The image, the user switch, the health check, and the README.

## Coverage

| ID | Where |
| --- | --- |
| R01 | Rendering |
| R02 | Rendering (class-based highlighting), `config/custom.css` |
| R03 | Layout template and CSS, which have no fixed-width elements |
| R04 | Offline |
| R05 | Data folder, the tree |
| R06, R07 | Changing pages |
| R08, R09 | Editor routes; a plain `<textarea>` with a Source and Preview tab |
| R10 | Uploads |
| R11 | Routes (owner only), Authentication |
| R12, R13, R14 | Authentication |
| C01–C06 | Data folder, Rendering and the cache, Configuration and deployment |
