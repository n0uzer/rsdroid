# Pinned dependencies

`deps.json` is the single source of truth for downloaded binaries. Every entry
pins a version, an HTTPS URL and a SHA-256 or SHA-512 checksum. `fetch-deps.ps1`
never executes a downloaded installer.

```powershell
# Fetch everything into %LOCALAPPDATA%\rsdroid
pwsh tools/fetch-deps.ps1

# Fetch only what a milestone needs
pwsh tools/fetch-deps.ps1 -Component qemu,scrcpy-server

# Verify files already on disk (no network)
pwsh tools/fetch-deps.ps1 -CheckOnly

# Validate the manifest only (used by CI)
pwsh tools/fetch-deps.ps1 -VerifyManifest
```

Layout under the data root:

```
%LOCALAPPDATA%\rsdroid\
  tools\qemu\qemu-w64-setup-20260811.exe
  tools\scrcpy\scrcpy-server-v5.0.1
  images\android-x86_64-9.0-r2.iso
```

QEMU is verified but not installed: whether rsdroid bundles QEMU or requires a
user install is an M1 decision. Install it to `C:\Program Files\qemu` or pass
`--qemu <path>` to `rsdroid doctor`.

Large files are never committed. To update a pin, change the version, URL, size
and checksum in `deps.json`, then run `pwsh tools/fetch-deps.ps1 -Force` and
record why in the pull request.
