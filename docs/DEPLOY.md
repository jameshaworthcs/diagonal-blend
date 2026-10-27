# Deploying diagonal blend

The site is static. GitHub Pages builds it with Actions and serves it at
**https://blend.cloud.jhaworth.ai**.

## Releasing

Build and test locally first (`make test`), then tag a release:

```sh
make deploy VERSION=v0.1.2   # tags HEAD and pushes the tag
```

Pushing a `v*` tag runs `.github/workflows/pages.yml`. It builds the core
(Rust 1.95, `wasm32-unknown-unknown`, wasm-bindgen 0.2.129) and the site,
checks that `dist/CNAME`, `dist/sw.js` and the wasm are there, and publishes
`dist/` with `actions/upload-pages-artifact` and `actions/deploy-pages`. A run
takes about three minutes.

To roll back, run the workflow by hand from an earlier tag:

```sh
gh workflow run pages.yml --ref v0.1.1
```

## Workflows

| Workflow | Runs on | Does |
| --- | --- | --- |
| `ci.yml` | `push` to `main`, `pull_request`, `workflow_dispatch` | Rust tests, unit tests, types, build, the Playwright suite (Firefox under `xvfb-run`). |
| `pages.yml` | `push` of tags `v*`, `workflow_dispatch` | Build and deploy, as above. |

Every action is pinned to a commit SHA, with its version in a comment.

## Setting up Pages

Once per repository:

1. **Build Pages with Actions.**
   ```sh
   gh api -X POST repos/OWNER/REPO/pages -f build_type=workflow
   ```
2. **Let tags deploy.** The `github-pages` environment only accepts the
   default branch at first, so a tag's deploy job is refused. Allow `v*`
   tags:
   ```sh
   gh api -X POST repos/OWNER/REPO/environments/github-pages/deployment-branch-policies \
     -f name='v*' -f type=tag
   ```
3. **DNS.** Point the domain at Pages with a CNAME to `OWNER.github.io.`.
4. **Custom domain.** `web/public/CNAME` ships in the build, but a site built
   by Actions takes its domain from the API:
   ```sh
   gh api -X PUT repos/OWNER/REPO/pages -f cname=blend.cloud.jhaworth.ai
   ```
5. **HTTPS.** Once GitHub has issued the certificate:
   ```sh
   gh api -X PUT repos/OWNER/REPO/pages -F https_enforced=true
   ```

## Checking it

```sh
dig +short blend.cloud.jhaworth.ai CNAME                    # jameshaworthcs.github.io.
curl -sI https://blend.cloud.jhaworth.ai/ | head -1         # HTTP/2 200
curl -sI http://blend.cloud.jhaworth.ai/ | grep -i location # https://...
```

Then in a browser: the page loads, *Try the sample twins* then *Align faces*
works, the service worker reports "Ready to work offline", and a reload with
the network off still works. `make lighthouse` measures the live site.
