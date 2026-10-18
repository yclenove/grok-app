# Browser extension store submission kit

Draft materials for maintainer review and submission to Chrome Web Store and
Microsoft Edge Add-ons. This directory is not a submission, a published listing,
or evidence of store approval. No store account, listing ID or screenshots have
been supplied. Do not submit documents containing unresolved placeholders.

## Materials

- [Chrome Web Store copy](chrome-web-store.md): description, single purpose and reviewer guidance.
- [Edge Add-ons copy](edge-add-ons.md): description, single purpose and reviewer guidance.
- [Permission justifications](permissions.md): the complete manifest permission inventory, shared by both listings.
- [Data use disclosures](data-use.md): local data flow and answers for the privacy questionnaires.
- [Privacy policy](privacy-policy.md): text to publish at the maintainer's policy URL.
- [User guide](../../../docs/browser-extension.md): installation, sharing, revocation and removal.
- [Extension implementation notes](../README.md): identity, transport and DOM limits.

## Maintainer inputs and release checklist

These placeholders are intentionally unusable; supply verified values before
publishing. Never generate or commit a private key as part of this kit.

| Required input | Placeholder | Maintainer action |
| --- | --- | --- |
| Fixed extension identity | `__MAINTAINER_EXTENSION_ID_FROM_MANIFEST_KEY__` | Supply the approved public manifest `key` through the identity release process; derive the ID from that key and verify `chrome.runtime.id` and native `allowed_origins` match it in each browser. Rebuild gcu and the extension together. No runtime ID override is supported. |
| Chrome developer account | `__MAINTAINER_CHROME_WEB_STORE_DEVELOPER_ACCOUNT__` | Use the project-controlled account and handle registration/fees outside this task. |
| Edge developer account | `__MAINTAINER_EDGE_ADD_ONS_DEVELOPER_ACCOUNT__` | Use the project-controlled Partner Center account outside this task. |
| Chrome listing URL | `__MAINTAINER_CHROME_WEB_STORE_LISTING_URL__` | Insert the actual public URL only when it exists. A listing identifier is not the native host identity. |
| Edge listing URL | `__MAINTAINER_EDGE_ADD_ONS_LISTING_URL__` | Insert the actual public URL only when it exists; verify the release identity rather than substituting the listing identifier. |
| Privacy policy hosting | `__MAINTAINER_PRIVACY_POLICY_URL__` | Host the policy at a public HTTPS URL readable without sign-in; insert it in both store forms and the user guide. |
| User guide hosting | `__MAINTAINER_USER_GUIDE_URL__` | Publish the user guide and retain its install/share/revoke/uninstall anchors; use the public URL in the hosted policy. |
| Publisher / data controller | `__MAINTAINER_PUBLISHER_LEGAL_NAME__` | Identify the entity responsible for this extension in both stores and the policy. |
| Privacy contact | `__MAINTAINER_PRIVACY_CONTACT__` | Supply a monitored contact channel and put it in the policy. |
| Policy effective date | `__MAINTAINER_PRIVACY_POLICY_EFFECTIVE_DATE__` | Set the date when the reviewed policy is published. |
| Support URL | `__MAINTAINER_SUPPORT_URL__` | Supply the public project support page for both listings. |
| Chrome screenshots | `__MAINTAINER_CHROME_WEB_STORE_SCREENSHOTS__` | Capture the actual release in a disposable profile using synthetic page content; meet the current store dimensions. |
| Edge screenshots | `__MAINTAINER_EDGE_ADD_ONS_SCREENSHOTS__` | Capture the actual release in Edge using synthetic content; meet the current store dimensions. |
| Store graphics | `__MAINTAINER_STORE_ICONS_AND_PROMOTIONAL_ASSETS__` | Provide approved icons/graphics required by the current forms; do not invent branded assets or reviewer evidence. |

- [ ] Resolve every placeholder above and in the materials; verify both browser identities against the approved public key. The checked-in manifest currently has no `key`, so installation and native startup fail closed.
- [ ] Verify the release's trusted native session/window binding, full ActionRouter dispatch and Stop lifecycle end to end. The current `gcu native-host` executable acknowledges consent only; core DOM fusion alone does not enable that entry's dispatch. Do not publish claims of working DOM control until the release actually wires and validates this path.
- [ ] Build the extension with the existing [build command](../README.md#build-and-checks). Package only the generated `dist/` extension files, not this submission kit, tests, private keys or development files.
- [ ] Review the [permission inventory](permissions.md) against the generated manifest. Run the native host and packaged-manifest tests below; any permission change requires a new justification and privacy review.
- [ ] Populate both privacy forms from [data use disclosures](data-use.md); local-only transmission is not an excuse to declare that no website content is processed.
- [ ] Publish the reviewed [privacy policy](privacy-policy.md) and user guide, resolve their public URL/store/publisher/contact/date placeholders and check the real URLs. Convert repository-relative links to their public destinations when hosting the guide. The offline tests check repository links, not placeholder destinations or store availability.
- [ ] Capture an idle tab, explicit toolbar sharing, revocation, and a synthetic authorized request if supported by the release. Show the real tooltip/badge and any actual approval UI. No mock screenshot or assumed review result substitutes for this evidence.
- [ ] Give reviewers a disposable fixture/profile and the matching gcu release. Run the installation/share/revoke/removal instructions in each browser, with no real accounts or credentials.
- [ ] The maintainer submits using the actual store accounts, answers any updated form requirements and records the real review outcome through the normal release process.

## Automated checks

From the repository root:

```sh
corepack pnpm exec vitest run computer-use/tools/browser-extension/tests/store.test.ts computer-use/tools/browser-extension/tests/dom.test.ts computer-use/tools/browser-extension/tests/native.test.ts
corepack pnpm exec tsc --noEmit -p computer-use/tools/browser-extension/tsconfig.json
python3 docs/plans/computer-use-v2/reviews/check_plan.py
```

From `computer-use/` on the authorized build machine:

```sh
rcargo test -p gcu-cli --test native_host
```

`store.test.ts` runs the real extension build and compares the packaged manifest
with the permission table and source manifest. The native host acceptance target
checks the same table, rejects added/removed/broadened permission fixtures, and
checks the kit's local links and the installation/share/revoke/uninstall anchors.
