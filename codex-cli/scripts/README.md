# npm releases

Use the DCCodex staging helper in the repo root to generate npm tarballs from a local musl
release artifact:

```bash
./scripts/stage_dccodex_npm_release.py --release-version 0.114.0
```

That writes publishable tarballs to `dist/npm/` for:

- `@pmcmick/dccodex`
- `@pmcmick/dccodex-linux-x64`

The generic helper still exists when you need to stage additional packages or reuse native
artifacts from a workflow run:

```bash
./scripts/stage_npm_packages.py \
  --release-version 0.6.0 \
  --package dccodex \
  --package codex-responses-api-proxy \
  --package codex-sdk
```

When `--package dccodex` is provided, the staging helper builds the lightweight
`@pmcmick/dccodex` meta package plus all platform-native `@pmcmick/dccodex` variants that
are later published as separate packages.

If you need to invoke `build_npm_package.py` directly, pass `--vendor-src` pointing to a
directory containing the populated `vendor/` tree.
