# me-wiki

A personal, self-hosted wiki that keeps its pages as Markdown files on disk. It needs no database, works on a phone
as well as a desktop, and can be read offline. [requirements.md](requirements.md) says what it does and why;
[tech-spec.md](tech-spec.md) says how.

## Run it with Docker

Released images for amd64 and arm64 are published to `ghcr.io/davidblkx/mewiki`:

```bash
docker run -d --name mewiki -p 8080:8080 -v /srv/mewiki:/data -e MEWIKI_PASSWORD='a long passphrase' ghcr.io/davidblkx/mewiki:latest
```

Tags follow the release: `0.1.0` for one release, `0.1` for the latest patch of that line, and `latest`. To build
the image yourself instead, run `docker build -t mewiki .` and use `mewiki` in place of the image name.

Everything the wiki stores lives in the `/data` volume, so backing it up means copying that folder. `cache/` inside it
can be left out of backups, since the app rebuilds it.

**Folder permissions.** The container starts as root only long enough to switch to `PUID`/`PGID`, `1000:1000` by
default, which is usually the first user on a Linux host. The host folder must belong to that user:

```bash
sudo chown -R 1000:1000 /srv/mewiki
```

To run as someone else, set `-e PUID=… -e PGID=…` and `chown` the folder to match. Starting the container with
`--user` skips the switch altogether. A named Docker volume, rather than a host folder, starts out owned by
`1000:1000`, so with any other `PUID` use a host folder; otherwise the server stops with "Permission denied".

**HTTPS is required for offline reading.** Browsers only run service workers over HTTPS, or on `localhost`. The app
itself speaks plain HTTP, so put a reverse proxy such as Caddy or Traefik in front of it.

## Settings

| Variable | Default | |
| --- | --- | --- |
| `MEWIKI_PASSWORD` | none, required | the only password; changing it logs every device out |
| `MEWIKI_DATA` | `/data` | the data folder |
| `MEWIKI_ADDR` | `0.0.0.0:8080` | listen address |
| `MEWIKI_COOKIE_SECURE` | `true` | set `false` only for plain-HTTP testing, or the login cookie won't stick |
| `PUID`, `PGID` | `1000`, `1000` | the user and group the server runs as |

## The data folder

```
pages/      the Markdown files; folders are the page tree
uploads/    uploaded files
cache/      rendered pages; safe to delete
config/     secret.key and custom.css
```

A page is `name.md`, and its subpages live in a `name/` folder next to it. Pages can be edited outside the app; they
re-render on the next visit. Pages added or moved outside the app appear in the tree after **Rebuild** on the
dashboard. Links aren't rewritten for files moved outside the app.

## Develop

```bash
MEWIKI_DATA=./data MEWIKI_PASSWORD=dev MEWIKI_COOKIE_SECURE=false MEWIKI_ADDR=127.0.0.1:8080 cargo run
```

Before committing:

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

CI runs the same checks on every push to `main` and on pull requests.

## Release

1. Set the new version in `Cargo.toml`, run `cargo check` so `Cargo.lock` follows, and commit both.
2. Tag that commit and push the tag:

   ```bash
   git tag v0.2.0 && git push origin v0.2.0
   ```

The Release workflow refuses a tag that doesn't match `Cargo.toml`. It then runs the checks, builds the image on
native amd64 and arm64 runners, publishes the version tags (and `latest`, except for prereleases such as
`v0.2.0-rc.1`), and creates the GitHub Release with generated notes.

The image is linked to this repository, so it is public like the repository and pulls without logging in. If a pull
ever asks for credentials, check the package's visibility in its settings on GitHub.
