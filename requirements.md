# me-wiki — requirements

## Context

Notes end up spread across tools, files and chats, so finding something already written down takes longer than it
should. A wiki gives that knowledge one place to live, organised as a tree of pages that can be read from anywhere.
Self-hosted wikis such as DokuWiki and Wiki.js already do this, but they take more setting up and looking after than
a personal wiki should need.

**me-wiki is a personal, self-hosted wiki that keeps its pages as Markdown files on disk, needs no database, and is
quick to edit and read from a desktop or a phone — and to read offline.**

## Users

- **Owner** — knows the password. Reads every page, creates and changes content, and uses the dashboard.
- **Visitor** — anyone else who can reach the server. Reads public pages only; protected pages cannot be opened.

## Requirements

### Reading

- **R01** Pages are written in Markdown, with tables, code highlighting, diagrams and task lists. *Why: plain text
  stays portable and editable with any tool.*
- **R02** Every page shares one look, set by a central stylesheet. The owner can add custom CSS from the dashboard,
  and it applies to every page.
- **R03** Pages read well on desktop and mobile browsers. On a desktop a page uses the full width beside the tree. A
  read mode, switched on per page, hides the tree and centres the page: text at about 75 characters a line, while
  code blocks, tables and diagrams can spread to 100rem. *Why: full width suits wide code and tables, but long lines
  of prose are hard to read.*
- **R04** The wiki can be installed as a PWA. Every page a reader opens is saved on their device and can be read
  offline afterwards. A sync button in the dashboard saves every page at once, protected ones included. Offline
  covers page text only: images and other uploaded assets, the editor and the dashboard need a connection. *Why:
  pages read once stay available on the go, and sync makes the whole wiki available before going offline.* Cost:
  protected pages the owner has opened or synced stay readable on that device to anyone who uses it, and logging
  out does not remove them.

### Organising

- **R05** Pages form a tree: any page can have subpages, and the navigation shows the tree.
- **R06** The owner can create, rename, move and delete pages. Deleting a page also deletes its subpages, after a
  confirmation that lists them. Moving a page out from under a protected page asks for confirmation too, listing
  the pages that will become public. *Why: protection is inherited (R13), so both actions can otherwise make pages
  public without the owner noticing.*
- **R07** Pages can link to other pages, and those links keep working when the target is renamed or moved. *Why:
  otherwise reorganising the tree silently breaks the wiki.*

### Editing

- **R08** The owner edits pages in the browser, without touching files on the server. The editor has two tabs: the
  Markdown source as plain text, and a preview of the rendered page.
- **R09** Editing works on a phone, not just on a desktop.
- **R10** The owner can upload assets of any type, up to 2 MB each, use them in pages, and delete them. Deleting a
  page leaves the assets it uses in place; the owner removes them separately.
- **R11** Every action that changes the wiki — create, edit, rename, move, delete, upload, and anything in the
  dashboard — requires login. *Why: without it, anyone who can reach the server can rewrite the wiki.*

### Access

- **R12** There is one password, read from the `MEWIKI_PASSWORD` environment variable. Changing it means restarting
  with a new value; there is no password screen in the dashboard. *Why: a single owner is enough for now, and one
  source of truth avoids a dashboard change being overwritten on the next restart.*
- **R13** The owner can mark a page as protected. Protection also covers the page's subpages, but never uploaded
  assets: an image or file used in a protected page can be opened by anyone who has its address. *Why: an asset's
  protection would otherwise depend on the pages using it, which changes as pages are edited or deleted (R10).
  Sensitive content belongs in the page text, not in an upload.*
- **R14** A visitor cannot open a protected page: its URL returns the same "not found" screen as a page that was
  never created. Its title can still appear in the navigation tree and in links from other pages. *Why: the content
  is what needs protecting; hiding every trace of the page is not worth the extra work.*

## Constraints

- **C01** No database. *Why: nothing extra to run, back up or migrate.*
- **C02** All data — pages, assets and settings — lives in a single folder, so a Docker deployment needs one volume
  and a backup is a copy of that folder.
- **C03** The Markdown files and their metadata are the source of truth. Everything the app generates from them,
  such as rendered pages, is a **cached asset** and can be rebuilt at any time.
- **C04** It runs as a single HTTP server, deployable as a Docker container.
- **C05** The page tree is the folder tree on disk; nothing else records which page is under which. *Why: the wiki
  stays browsable and editable outside the app (C03), and there is no second copy of the structure to drift.* A
  per-page parent field in the metadata was considered and dropped as too complex for what it adds.
- **C06** Cached assets live in a folder of their own inside the data folder, apart from the pages and uploaded
  assets. *Why: the cache can be deleted and rebuilt without touching content, and the page folders stay clean for
  editing outside the app (C05).*

## Out of scope for the first version

- Search.
- Page history and restoring old versions.
- More than one user, or per-user permissions.
- Editing while offline.
- Changing the password from the web.
- Detecting conflicting saves. If the same page is saved from two devices, the last save wins without a warning.

## Notes for the tech spec

Design decisions from the first draft, to be worked out in the tech spec rather than here:

- **Storage per page.** Each page is a `.md` source and a `.json` with its metadata (such as whether it is
  protected) — but not its place in the tree (C05). The pre-rendered `.html` served in read mode is a cached asset
  (C06). It is rebuilt on every save, and a dashboard action rebuilds every page — for example after the custom CSS
  changes.
- **Folder layout.** How a page and its subpages map to files and folders — for example `mushroom.md` with its
  subpages in `mushroom/` — where uploaded assets live, and what renaming or moving a page does on disk (R06, R07).
- **Login.** The password is hashed with a key generated on first run; a successful login sets an HTTP-only cookie.
  As drafted, the cookie holds a value derived from the password, so it works the same as the password itself. It
  never expires and can only be revoked by changing the password. The spec should cover session tokens, expiry,
  and the hashing algorithm.
- **Protection checks** run on the server before a protected page is served (R13, R14). Uploaded assets are never
  checked.
- **Uploaded files.** Uploads of any type (R10) include HTML and SVG. If those are served as-is from the wiki's own
  address, a page in them can run scripts as the logged-in owner. The spec should decide how uploads are served so
  they cannot.
