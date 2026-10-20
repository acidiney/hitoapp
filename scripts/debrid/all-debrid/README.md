# AllDebrid standalone package

This packages the accepted metadata service as a separate process. The packaging
and audit below are drafts until an actual image build and offline audit pass.
They do not qualify live AllDebrid, audio formats, active-media shutdown, or a
Hito account connection.

The runtime image contains the selected release binary and licenses. Its default
user is `10001:10001`; Compose gives it a read-only root and private input volume,
drops all capabilities, denies privilege escalation, and limits CPU, memory,
PIDs, and retained logs. The normal port mapping is `127.0.0.1:8097:8097`.
The public manifest supplies health without an authorization header.

## Freeze and build

Run commands from the worktree root. `source.py` only reads source and creates a
new private snapshot directory. It records every frozen file's bytes and mode,
the base Git revision, and a digest covering that inventory. The snapshot also
includes the audit policy. No credentials or operator catalog belong in source.

```sh
python3 scripts/debrid/all-debrid/source.py freeze /tmp/hito-all-debrid-package-SNAPSHOT
python3 /tmp/hito-all-debrid-package-SNAPSHOT/context/scripts/debrid/all-debrid/source.py verify /tmp/hito-all-debrid-package-SNAPSHOT/source.json
python3 /tmp/hito-all-debrid-package-SNAPSHOT/context/scripts/debrid/all-debrid/build.py --snapshot /tmp/hito-all-debrid-package-SNAPSHOT/source.json --check-only
```

Before a build, hold the global compiler token and check host free space and
combined Hito Cargo/BuildKit target usage. Retain at least 40 GiB of host free
space. Stop starting checks at 9 GiB; the combined hard limit is 10 GiB. Include
the new release cache and the registry reserve when accounting for this build.
Do not remove active caches or application data.

```sh
python3 /tmp/hito-all-debrid-package-SNAPSHOT/context/scripts/debrid/all-debrid/build.py --snapshot /tmp/hito-all-debrid-package-SNAPSHOT/source.json
```

Use this build helper for the image build. It derives the context, image tag,
revision, input digest and cache identity from the verified manifest, replacing
inherited source and Compose settings. Before compiling, it resolves Compose
configuration and requires those exact values, including the Dockerfile. The
build uses the same command and environment as that check. Source verification
runs before configuration, immediately before build, and after build. The
`--check-only` command resolves this configuration without starting a compiler.

The pinned Rust and Debian base images follow the reviewed Real-Debrid package.
Cargo is locked, offline, limited to two jobs, and builds only the release
executable with debug information and incremental compilation disabled. The
registry cache must already contain its dependencies. The separate release
target cache includes the input digest and target architecture. Debian package
installation during image construction requires its package repositories; the
runtime audit below has no network access.

Resolve the resulting local tag to its immutable `sha256:` image ID with
`docker image inspect`. The tag is `hito-plugin-all-debrid:` followed by
`input_sha256` from `source.json`. The audit checks that exact image ID and its
source revision and input-digest labels.

## Offline image audit

Only run after the image build is authorized and terminal. Supply the actual
image ID; use a new output directory.

```sh
python3 /tmp/hito-all-debrid-package-SNAPSHOT/context/scripts/debrid/all-debrid/audit.py --image-id sha256:IMAGE_DIGEST --snapshot /tmp/hito-all-debrid-package-SNAPSHOT/source.json --output-dir /tmp/hito-all-debrid-package-AUDIT
```

The audit creates one unique labeled volume and four uniquely named containers
through its dedicated Compose file. The image itself provisions synthetic input
bytes from an in-memory tar archive, then inspects them before and after use.
Only this provisioning helper runs as root with the three capabilities needed
to set file ownership and modes. The service drops every capability and mounts
that volume read-only. Both helpers and service use `network_mode: none`; no
service has published ports. HTTP probes execute curl inside the service's
network namespace against loopback. Tokens pass through stdin, never command
arguments or environment variables.

The audit waits for real manifest health, checks the public manifest, rejects
missing/wrong/provider credentials, and compares authenticated search and detail
with the exact owned recording. It issues no source, preparation, provider, or
audio request. It requires idle SIGTERM to join with exit 0 and no OOM kill.
Complete combined stdout/stderr TRACE output must fit 64 KiB and contain only
the exact safe lifecycle/request fields, UUID correlations, timings, and expected
outcomes. Manifest health requests have an explicit finite allowance. Private
file bytes and descriptor metadata, including inode, owner, link count, mode,
size, mtime and ctime, must remain identical; access time is excluded.

Every Docker command has a deadline. Cleanup verifies the unique owner label,
Compose project, image ID and exact resource ID before stopping/removing each
container; it verifies the volume label before removal. It never invokes
`compose down`, force-removes a running container, prunes caches, or removes an
operator volume. Stop and cleanup failures fail the audit and remain in
`result.json`. A forced termination of the audit process or Docker failure can
leave owned resources; use the exact owner/IDs in that result to inspect and
recover them. Do not broaden cleanup to another project.

`result.json` records commands, immutable identities, safe outcomes and cleanup
results. `service.jsonl` retains the bounded service logs. The new output
directory is private; neither file contains the synthetic request bodies or
credential files. Source inputs are verified before and after execution.

## Operator inputs and approved stop

Provision a separate private volume outside Git. Its root must belong to
`10001:10001` with mode `0700`. Supply these four distinct regular, single-link
files, owned by UID 10001 with mode `0400` or `0600`:

| File | Content |
| --- | --- |
| `all-debrid-provider-token` | Private AllDebrid token, separate from the service token |
| `all-debrid-service-token` | Private bearer used by Hito to authenticate to this service |
| `all-debrid-receipt-key` | Persistent, exactly 32 binary bytes, not all zero |
| `all-debrid-catalog.json` | Private operator-curated catalog accepted by the executable |

Tokens may end in one LF or CRLF. Keep the receipt key stable across restarts.
Do not pass any of these bytes inline, in Compose environment variables, or in
Git. The audit's owned synthetic volume is discarded and is not an operator
provisioning tool.

Once the package is qualified and deployment is separately authorized, set
`HITO_ALL_DEBRID_CREDENTIAL_VOLUME` to the provisioned volume and use the frozen
Compose file with the source revision and digest from `source.json`, the digest
as the image tag, and `up -d --no-build --pull never`. The default
publication remains loopback. Use Compose `stop plugin-all-debrid` for SIGTERM
with its 30-second grace; inspect exit status and the stopped lifecycle event.
Only the idle stop is covered by this audit. Active-media drain and normal Hito
integration remain separate release work.
