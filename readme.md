# Letterboxd List Sync

A small Rust service that mirrors a public Letterboxd list into a list owned by
your account. GitHub Actions runs it once an hour. Additions, removals, and order
changes in the source are copied to the target.

This repository is configured to mirror
[`cokewithglitter/a&a: vampire season`](https://letterboxd.com/cokewithglitter/list/aa-vampire-season/).

## Configuration

Edit [`config.yaml`](config.yaml):

```yaml
username: shadrunov

lists:
  - source: https://letterboxd.com/cokewithglitter/list/aa-vampire-season/
    target: "a&a: vampire season"
```

Set `username` once, then add a `source` URL and target list name for every list
you want to mirror. The target URL is derived from the username and name. A
missing target is created on the first successful run.

Mirrored lists are always public and unranked. Their description links back to
the source as `synced from <source URL>`.

## Authentication

Letterboxd's official API is request-only, and its access page says personal
projects are not currently granted access. This service therefore uses your
revocable “remember me” web session, never your password.

Log in to Letterboxd in a desktop browser with **Remember me** enabled. Open
Developer Tools, select **Application** (Chrome) or **Storage** (Firefox), open
Cookies for `https://letterboxd.com`, and copy the values of:

- `letterboxd.user`
- `com.xk72.webparts.csrf`

In the GitHub repository, open **Settings → Secrets and variables → Actions**
and create these repository secrets:

- `LETTERBOXD_USER_COOKIE`
- `LETTERBOXD_CSRF_COOKIE`

Treat both values like a password. Do not paste them into an issue, commit, or
workflow file. Logging out of Letterboxd revokes the session; update both
secrets when it expires.

## Run

```sh
cargo run -- --dry-run
cargo run
```

The dry run only reads the public source lists. The normal run uses the session
cookies to create or update lists owned by `username`.

GitHub Actions runs at minute `17` of every hour and can also be started from
the Actions tab. Concurrent runs are serialized.

## Caveat

Letterboxd does not publish a supported personal automation API. The
authenticated write path can change without notice. CI tests protect the
parser and configuration, while every sync fails closed on authentication,
ownership, version, or API errors.
