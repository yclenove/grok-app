# Wallpaper Generation Capability Evidence

Checked: 2026-09-07. Scope: installed CLI version and read-only local upstream source inspection. No network update, paid API request or live generation was performed for this check.

## Version Boundary

- Installed command: `C:/Users/Administrator/.grok/bin/grok.exe --version` reports `grok 1.0.13 (5e9a58528b76)`.
- Local upstream checkout: `H:/aicoding/grok-build`, commit `9684fa3cdbf2995e30ea8b9b637f1db008f144fc`.
- These identifiers differ. The source provides a concrete adapter reference, not proof that every installed tool or account supports every field. Installed-schema and representative runtime validation remain required before expanding product controls.

## Tool Contracts

| Tool | Request fields / behavior in inspected source | Product consequence |
| --- | --- | --- |
| `image_gen` | `prompt`, `aspect_ratio` (default `auto`); request payload uses `resolution: 1k` | Keep image ratio selection. No request-level resolution or model selector is exposed by this input type. |
| `image_edit` | `prompt`, `image[]`, `aspect_ratio`; single-image edits ignore the ratio field and preserve input ratio; multi-image edits send the ratio | The application's explicit-ratio adapter uses duplicate unchanged references. Only the previously observed 16:9 result has real-output evidence; other ratios still need representative verification. |
| `image_to_video` | `image`, optional `prompt`, `duration` restricted to 6 or 10, `resolution_name` restricted to 480p or 720p; fixed `grok-imagine-video-1.5` | Do not add a request-level ratio/model control or 1080p option to this tool. Verify produced dimensions and duration separately. |
| `reference_to_video` | Required prompt, up to 7 reference images and/or up to 3 preset voices; ratio; duration 1-15 seconds; 480p or 720p | A possible separate workflow, not a transparent replacement for animating an image as the first frame. Not currently implemented or validated in the wallpaper module. |

Image generation and editing default to `grok-imagine-image-quality`. `ImageGenClient` accepts separate nonempty configuration-level `model_override` and `edit_model_override` values. This mechanism does not enumerate account entitlements or prove that arbitrary models are supported on the current subscription route. No model configuration was changed.

Image-edit ratio documentation lists `auto`, `1:1`, `16:9`, `9:16`, `4:3`, `3:4`, `3:2`, `2:3`, `2:1`, `1:2`, `19.5:9`, `9:19.5`, `20:9`, and `9:20`. A documented string value is not evidence that a particular generated bitmap honored it.

## Source Pointers

All links pin the inspected source commit:

- [Video model, validators and input types](https://github.com/xai-org/grok-build/blob/9684fa3cdbf2995e30ea8b9b637f1db008f144fc/crates/codegen/xai-grok-tools/src/implementations/grok_build/video_gen/mod.rs): model at line 36; duration validation at 925; input types at 970 and 1000; a test explicitly rejects 1080p at 1664.
- [Image generation input and model configuration](https://github.com/xai-org/grok-build/blob/9684fa3cdbf2995e30ea8b9b637f1db008f144fc/crates/codegen/xai-grok-tools/src/implementations/grok_build/image_gen/mod.rs): model defaults at 31, configuration resolution at 98, request payload at 246, input at 379.
- [Image editing input and ratio behavior](https://github.com/xai-org/grok-build/blob/9684fa3cdbf2995e30ea8b9b637f1db008f144fc/crates/codegen/xai-grok-tools/src/implementations/grok_build/image_edit/mod.rs): default model at 28, input at 227, payload and single/multi-image behavior at 358.

## Remaining Acceptance

1. Inspect installed tool contracts when available and compare against these pinned inputs; keep unknown differences explicit.
2. Record the application's admitted parameters and audited arguments for all three existing modes. Preserve the user's source and prompt on failure; never retry generation automatically.
3. Probe produced video dimensions and duration, then validate representative outputs against selected options. Keep requested settings separate from observed media properties and actual-model claims.
4. When an output ratio requires composition changes, offer an explicit edit-then-video workflow with both generation steps visible. Do not stretch the image to simulate a ratio change.
5. Document any separately proposed paid API route and its billing before enabling it. Neither the tool schema nor an API response proves that API billing consumes subscription quota; HTTP 429 alone does not establish quota exhaustion.
