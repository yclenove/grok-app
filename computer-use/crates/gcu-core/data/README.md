# Policy Data Reference

This directory contains static policy datasets for Grok Computer Use v2 (`gcu-core`), implementing security boundaries defined in [Architecture Constitution §7.4, §7.5](../../../../docs/plans/computer-use-v2/03-architecture.md#7-安全与授权模型).

---

## 1. Directory Structure

```text
computer-use/crates/gcu-core/data/
├── denylist.toml               # Platform-specific denylists of prohibited applications
├── sensitive_terms/            # Multilingual sensitive action dictionaries (15 locales)
│   ├── de.txt
│   ├── en.txt
│   ├── es.txt
│   ├── fil.txt
│   ├── fr.txt
│   ├── id.txt
│   ├── it.txt
│   ├── ja.txt
│   ├── ko.txt
│   ├── pt-BR.txt
│   ├── ru.txt
│   ├── ta.txt
│   ├── uk.txt
│   ├── zh.txt
│   └── zh-TW.txt
└── README.md
```

---

## 2. Denylist (`denylist.toml`)

### Format

`denylist.toml` defines prohibited applications across three desktop target platforms: `[[macos]]`, `[[windows]]`, and `[[linux]]`.

Each entry is a TOML table containing three required fields:

| Field | Type | Description |
|---|---|---|
| `id` | `string` | Platform-specific primary identifier. |
| `category` | `string` | One of `grok_self`, `terminal`, `credentials`, `security_prompt`, `security_software`. |
| `match` | `string` | Match algorithm. Currently `exact`. |

### Category Taxonomy

1. **`grok_self`**: Grok desktop application itself, Grok CLI hosts, and `gcu` helper processes to prevent self-targeting and control loops.
2. **`terminal`**: Terminal emulators and interactive shell hosts to prevent arbitrary command execution bypassing policy barriers.
3. **`credentials`**: Password managers, credential stores, keychains, and token vaults.
4. **`security_prompt`**: Operating system privilege elevation prompts, UAC dialogs, biometric auth prompts, and Polkit agents (`handoff_required`).
5. **`security_software`**: Security consoles, EDR agents, and antivirus configuration interfaces.

### Platform Identifiers & Match Semantics

- **macOS (`[[macos]]`)**:
  - Identifier: Bundle Identifier (e.g., `com.apple.Terminal`, `com.grokapp.desktop`).
  - Semantics: Case-sensitive exact match against the target application's bundle identifier.
- **Windows (`[[windows]]`)**:
  - Identifier: Executable file name in lowercase (e.g., `windowsterminal.exe`, `powershell.exe`).
  - Semantics: Case-insensitive exact match against the target process image name normalized to lowercase.
- **Linux (`[[linux]]`)**:
  - Identifier: Desktop entry file ID (e.g., `org.gnome.Terminal.desktop`) or binary executable name (e.g., `gnome-terminal`).
  - Semantics: Exact match against the resolved desktop file name or `/proc/<pid>/exe` basename.

> **Curation Policy**: Every identifier must correspond to an authentic, verified application release. Never invent or speculate identifiers.

---

## 3. Sensitive Action Terms (`sensitive_terms/<locale>.txt`)

### Format

- Plain text files encoded in **UTF-8**, ending with a single newline (`\n`).
- Lines beginning with `#` are category section headers or comments.
- 15 shipped locales: `de`, `en`, `es`, `fil`, `fr`, `id`, `it`, `ja`, `ko`, `pt-BR`, `ru`, `ta`, `uk`, `zh`, `zh-TW`.
- Each file covers 10 standard semantic categories:
  - `send`: Message and email sending.
  - `pay`: Payment and checkout actions.
  - `delete`: Deletion, discarding, or moving to trash.
  - `submit`: Form submission and order confirmation.
  - `transfer`: Funds transfer and remittances.
  - `sign`: Digital signing, contract signing, and agreement execution.
  - `publish`: Content publication and public posting.
  - `confirm_purchase`: Purchase confirmation and order placement.
  - `password_change`: Password change, credential reset, or security credential updates.
  - `logout_all`: Global session termination or signing out of all devices.

### Match Semantics

When evaluating a UI element (e.g., button, menu item) during an action request:
1. Extract accessible labels: accessibility name (`AXTitle` / UIA `Name` / AT-SPI `name`), description, and role.
2. Normalize target string using **Unicode NFKC normalization**.
3. Apply **case-folding** (Unicode casefold / lowercase conversion).
4. Perform whole-word or whole-phrase inclusion matching against dictionary terms of the active/detected locales.
5. If matched and risk is high, route through `ApprovalProvider.confirm_action`.

---

## 4. How to Add New Entries

### Adding to `denylist.toml`

1. Confirm the real, verified application identifier from the vendor's application package (`Info.plist`, PE header, or `.desktop` file).
2. Append the entry to the corresponding platform section (`[[macos]]`, `[[windows]]`, or `[[linux]]`).
3. Set `category` to one of the five approved categories.
4. Set `match = "exact"`.

### Adding to `sensitive_terms/<locale>.txt`

1. Identify the target locale file in `sensitive_terms/<locale>.txt`.
2. Locate the appropriate `# <category>` section header.
3. Add the authentic UI button text in lowercase-friendly UTF-8.
4. Ensure no duplicate entries and no trailing whitespace.

### Validation

Before committing, validate data integrity using the provided validation scripts:

```bash
# Validate policy data
python3 -I computer-use/scripts/check_policy_data.py

# Run test suite
python3 -I -m unittest computer-use/scripts/test_check_policy_data.py
```
