# Disposable desktop VM checks

Use a clean macOS, Windows or Linux desktop VM with Python 3.12+ and no real
accounts or credentials. No package/runtime is installed by these scripts.
Capture the terminal result and permission UI for the release review.

```
python3 run.py --archive /path/to/gcu-VERSION-TARGET.tar.gz --sums /path/to/SHA256SUMS
# Windows: py -3 run.py --archive C:\path\gcu-VERSION-TARGET.zip --sums C:\path\SHA256SUMS
```

The script verifies hashes and archive paths, extracts into a disposable directory,
checks the version/signature/helper identity, installs Grok MCP registration in a
new project twice, calls native doctor/MCP initialize/tools/list/capabilities,
uninstalls registration, checks idempotent removal, and removes the install directory.
`--grok /absolute/path/to/grok` additionally runs `grok mcp list` against that empty
home/project; no model call or login. macOS native testing registers the helper via
Launch Services; approve TCC manually, then revoke the grant in System Settings and
reset the VM after testing. The script never grants/revokes system permissions.
`--package-only` explicitly reports native doctor/MCP as **not_run**; it is not
sufficient for the P4-07 native installation DoD. Current CLI native assembly is a
known prerequisite; do not add `--driver fake` to make a native test pass.

On each VM also exercise the relevant package manager: npm install/uninstall the
local hashed tarballs; brew install/uninstall the generated formula on macOS;
winget validate/install/uninstall the generated local manifest on Windows;
apt/dnf install/remove the local deb/rpm on Linux. Inspect permissions, executable
modes and helper registration. Package-manager installs require VM admin approval
and registry/release availability, so these are manual acceptance steps.

Negative checks (must fail before running an executable): alter one archive byte,
remove its checksum record, use a mismatched target archive. For SBOM/signing:
validate the CycloneDX document, compare `release.json` target/version, run
`cosign verify-blob` with the documented exact workflow/tag identity; macOS
`codesign --verify --deep --strict`, signed builds `xcrun stapler validate`;
Windows `Get-AuthenticodeSignature` (Valid only if configured, NotSigned otherwise).
Record actual platform results; missing runners, certificates or desktop grants
are **not_run**, never substituted with fixture/native-driver source tests.
