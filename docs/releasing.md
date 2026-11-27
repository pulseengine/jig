# Releasing jig

jig cuts signed releases (owner decision, 2026-11-21) so that varve can carry
it in the pulseengine layer and callers can run a binary whose digest they
pinned.

1. Bump `version` in Cargo.toml, merge through the queue, and wait for main CI
   to succeed on the merge commit.
2. Tag that commit with a signed, annotated `v<version>` tag and push the tag.
   `.github/workflows/release.yml` builds the three archives, the
   wasm32-wasip2 component (checked under wasmtime against native jig first,
   REQ-030), the SBOM and the docs archive, signs SHA256SUMS.txt keyless with
   cosign and attests the archives and the component. It refuses a tag that
   does not name the Cargo.toml version.
3. Verify the release from outside the workflow:
   `cosign verify-blob --certificate-identity
   'https://github.com/pulseengine/jig/.github/workflows/release.yml@refs/tags/v<version>'
   --certificate-oidc-issuer https://token.actions.githubusercontent.com
   --bundle SHA256SUMS.txt.cosign.bundle SHA256SUMS.txt`.
   Then `sha256sum -c --ignore-missing SHA256SUMS.txt`, `gh attestation verify`
   on the x86_64 musl archive and on `jig-v<version>-wasm32-wasip2.wasm`, and
   `wasmtime run jig-v<version>-wasm32-wasip2.wasm --version`.
4. In a follow-up PR, record the verified x86_64-unknown-linux-musl digest as
   `JIG_VERSION` / `JIG_SHA256` in baseline.yml, required.yml and action.yml.
   Callers pin the merge commit of that PR, not the tag: it is the first
   commit whose workflows can fetch the released binary.
5. Ask the varve session to ingest the release into the pulseengine layer.

The version stays below 1.0 until every pulseengine repository with CI calls
jig in enforce mode (REQ-020).
