Verdict: **ACCEPT**

Round-2 independent DESIGN acceptance review of ADR 0019 on `cu2/design-mailbox`, HEAD `2aaca70245bbcf9e692937160a0940a52cc8c0a5` (2026-11-10). This accepts the design with its explicit measurement and admission gates; it does not assert that the proposed macOS operations or timing work. There are **no remaining must-fix findings** under this round's criteria. Findings 13–15 are should-fix improvements and do not condition acceptance.

I read the unchanged round-1 report first, then the revised ADR 0019, ADR 0018 §1/§2/§5/§13/§14, the relevant ADR 0017 constraints, acceptance matrix §8, and the guest/machine interfaces and dispatch, cleanup and recovery code. References below are relative to `computer-use/` unless marked repo root; ADR 0019 line numbers refer to this HEAD. The owner decisions remain final: no production IP control channel; Softnet block-all including gateway plus approved endpoint `/32`; no host guard; only the dedicated mailbox export; host volume operations only after the required design review.

**Round-1 findings 1–12: disposition and remaining implementation risks.** “Fixed” below means the principle is now stated sufficiently for implementation and acceptance testing, not that implementation or native evidence exists.

1. **Round-1 #1 — Fixed at the root: publication retains a protected source identity.** §2 lines 23–25; §3 lines 70–76; C3 line 130; M3 line 151. Previous severity: must-fix; remaining design severity: none.

   Exact text: “同卷、不导出且 guest 不可达” and `renameat(stage_fd,tmp,host_write_fd,slot)` (line 72). The source parent is outside the sole export, the created FD survives write/fsync/publication, the destination identity is checked, and replacement does not follow its target. `fstatfs(fd)` now supplies FSID explicitly. C3 includes “open 后 tmp 换名/替身”.

   An implementer could still stage under the exported tree, close the FD before publication, or mistake a pre-rename destination check for an atomic guarantee. Those recreate the substitution scenario. Minimal fix: implement the specified protected staging boundary and C3 race; no further design mechanism is needed. Export traversal/alias tests must pass before relying on that boundary.

2. **Round-1 #2 — Fixed at the root: pathname destruction has an explicit effect-time trust assumption.** §2 destructive checklist lines 51–54; C5 line 132; M5 line 153. Previous severity: must-fix; remaining design severity: none.

   Exact text: “检查不等于生效时绑定”, “从映射核验到操作完成，无其它手工/自动磁盘 attach/detach、热插拔或目标路径替换”, and otherwise “不发破坏性 argv，隔离” (line 51). This directly covers both reused `/dev/diskN` and parent-FD unlink's remaining child-name race; repeated queries are expressly insufficient.

   An implementer could still treat the eval lock, a fresh UUID query, or an informal quiet period as proof of exclusion. Minimal fix: establish and record the stated trusted-host exclusion for the actual operation window, or quarantine/use an independently measured identity-bound operation. This is an explicit conditional boundary, not evidence that all host mutation can already be excluded. Do not invent a daemon or host guard to satisfy it.

3. **Round-1 #3 — Fixed at the root: creation intent and persisted child identity are separate.** §2 lines 25, 52–55; C5 line 132; M5 line 153. Previous severity: should-fix; remaining design severity: none.

   Exact text: “未挂载的底层 M₀”, “S 的每个固定目录、marker 文件也逐项记录”, and “只有意图无子 inode … 均隔离”. The mounted root V and covered M₀ are explicitly different objects. Image unlink additionally requires absence of every attachment/mount, including an image not known to have attached successfully.

   An implementer could still reconstruct ownership from the intended name after a lost mkdir reply, or compare M₀ to V after detach. Minimal fix: persist and use the listed parent/child identities, never recursively claim a replacement, and retain an intention-only orphan when reconciliation cannot identify it. M5 now owns these cases.

4. **Round-1 #4 — Fixed at the root: one caller, bounded scheduling gaps, saved absolute operation ends.** §2 lines 57–66; §3 lines 100–106; M4 line 152. Previous severity: must-fix; remaining design severity: none.

   Exact text: “每命令 begin 一次，每 step 只 poll 一个有界片后返回” and “全部排队、IPC、解析与 IO 的总间隔受预算表100ms约束” (line 102). The loop covers reserve through seal, including volume work, probes and slow cleanup; line 104 saves `op_end`, names the Task aggregate, clips subsequent queries, and discards late success before further dispatch. Owner proofs continue through cleanup and cannot be generated from local `now`.

   An implementer could still run all 16 probes in one call, block four seconds inside stop, reset the operation end on each inventory poll, or confuse a polling slice with the caller's absolute operation deadline. Minimal fix: implement the single-loop contract and required expiry/kill/reap traces. Existing `machine/io.rs:117` has no Task aggregate; `steps.rs:289` still loops synchronously. These are explicitly identified implementation work, not evidence that the contract already holds. Line 106 closes real admission if filesystem blocking cannot satisfy it.

5. **Round-1 #5 — Fixed at the root: issued-but-unseen obsolete lease replies have bounded treatment.** §3 lines 89, 94, 98; M3/M4 lines 151–152. Previous severity: must-fix; remaining design severity: none.

   Exact text: “即使首次见到也只丢弃，不延长任何截止” for an old ACK matching an actually issued grant in this boot/attempt (line 94). The bounded table records `(seq,inventory_request_seq,sample_ms,until_ms)`; inconsistent, future or never-issued frames still fail. Line 98 defines the boot-local continuous monotonic millisecond sample, genuine sample association, checked arithmetic, pause/resume gate and irreversible expiry.

   An implementer could still reject a delayed legitimate ACK as a wrong request, renew from it, or start a fresh two seconds at delivery. Minimal fix: apply the issued-grant lookup and fixed `g+2000ms` rule, including skipped observations. Visibility near one second each way remains a feasibility concern, not proof of sufficient margin. C1/M7/M9 must reject insufficient margin without increasing lease or Stop constants. See finding 14 for request-rate clarity.

6. **Round-1 #6 — Fixed at the root: progress, completion and authority are distinct for all seven methods.** §3 lines 78–94; M3/M4 lines 151–152. Previous severity: should-fix; remaining design severity: none.

   Exact text: “pending 是本地未完成状态”, “同一逻辑操作重复 poll 不再发命令”, and “accepted/pending 不消费”. The operation table gives separate inventory/probe/bootstrap/login/run/lease/revoke contracts; retained terminal results are acknowledged through later inventory consumption fields. Revoke acceptance is explicitly not drain; bootstrap's disk field is explicitly not native ACK.

   An implementer could still map a missing slot to `CallError`, consume on acceptance, acknowledge before the machine records the terminal, or leave immediate-success parsers in place. Minimal fix: implement the entire M4 transition/consumption contract over the existing bounded byte return; the illustrative `BoundedOutput {state:…}` is a normalized body, not permission to expose a new launch capability or expand `GuestChannel`. Current `steps.rs:333`, `:361`, `:437`, `io.rs:245` and `cleanup.rs:81` are insufficient as-is, as the ADR now states.

7. **Round-1 #7 — Fixed at the root: the mechanics probe no longer requires the future supervisor.** §4 line 110; §5 lines 116, 122, 124, 128, 134; M2/M7/M9 lines 150, 155, 157. Previous severity: must-fix; remaining design severity: none.

   Exact text: “不依赖 M7 supervisor/golden/sandbox” and “C7 自主终止及所有 transport/liveness 验收移到 M7/M9”. M2 can collect file/volume/cache measurements independently. Missing second-principal permissions are denied/unknown or deferred, rather than solved with guest sudo.

   An implementer could still count SSH-driven termination as autonomous expiry evidence, or demand M7's sealed supervisor before M2 can measure anything. Minimal fix: keep the stated evidence split and run autonomous tests with harmless substitute trees under block-all in M7/M9. The pre-installed-program setup is now an avoidable convenience restriction, addressed in finding 13; it is not a dependency on S11e.

8. **Round-1 #8 — Fixed at the root: DEFAULT SSH is an explicit, limited experiment.** §1 line 9; §5 lines 116–124; §6 lines 143, 150, 155–157. Previous severity: should-fix; remaining design severity: none.

   Exact text: “key 认证不限制 egress，丢弃 clone 不能撤销已发生的外部影响” (line 118), and “DEFAULT-network / mechanics-only” (line 122). The exact DEFAULT argv, host-key/clone/IP/boot binding, isolated SSH settings, key-only authentication, disabled forwarding/interactive paths, bounded output and kill/reap are present. SSH supplies neither the measured lease nor authoritative file/mount/cleanup evidence.

   An implementer could still call DEFAULT networking host isolation, quietly use it in production, or count SSH stdout as a passing filesystem/liveness result. Minimal fix: preserve the experimental scope and evidence labels; repeat the required tests under block-all. ADR 0018 §14 supersedes its earlier H1/H2 proposals; the laboratory exception does not reopen production H1 or authorize a host guard. Finding 13 only changes how the fixed laboratory artifact arrives.

9. **Round-1 #9 — Fixed at the root: `-srcfolder` no longer proves a safe first mount.** §2 lines 31, 41–45; C4/C10 lines 131, 137. Previous severity: should-fix; remaining design severity: none.

   Exact text: “预置 marker 不证明任何首次挂载前已存在” and “若必须限制所有内部挂载而此配方做不到，就拒绝该配方” (line 41). Tool-internal devices, mounts, scratch and service activity are measured separately from the controlled attach. Hostile bytes are exported only after controlled-mount checks pass.

   An implementer could still inspect only the later attach and assume create had no earlier mount. Minimal fix: execute C4/C10's stage coverage and mark missing observations unknown; do not silently rename the later mount “first”. No new filesystem recipe is warranted before those measurements.

10. **Round-1 #10 — Fixed at the root: argv semantics and privilege are gates, not assumptions.** §2 lines 35–47; C4/C6/C10 lines 131, 133, 137. Previous severity: should-fix; remaining design severity: none.

    Exact text: “实际 eval 用户、无 root/sudo、无交互授权” and “GUI 授权/Full Disk Access 要求、忽略限制、证据不可读均阻断”. The checklist separates inspection from prohibited mutations, checks normal/force detach authority and whole-device mapping, and forbids opportunistic `tmutil -p/-v` substitutions or global indexing changes.

    An implementer could still equate exit zero with effective exclusions/flags, trigger a GUI authorization wait, or use unreadable service evidence as a pass. Minimal fix: collect the specified bounded readbacks as the eval account and reject unsupported/unobservable cases. The recipe remains conditional; no macOS 27 option or authorization claim was established by this document review.

11. **Round-1 #11 — Fixed at the root: finite subcases and incomplete-result semantics are specified.** §2 line 21; §5 lines 124, 128–137. Previous severity: should-fix; remaining design severity: none.

    Exact text: “子例独立 attempt”, “未完成记 unknown，watchdog 不是文件系统失败”, and “少量样本不报可信 p99”. C1 has eight samples per direction; C3 names the publication/open substitution windows; C5 covers lost effects and retargeting; C9 has finite write/file/churn bounds plus both image size and allocated blocks; C10 requires actual observation coverage and backup activity, not an idle-window inference.

    An implementer could still combine all crash points into one 60-second attempt, subtract timestamps from different monotonic clocks, claim sparse-image peaks prove a universal storage bound, or report no backup solely because no job ran. Minimal fix: preserve the independent attempts, use same-clock elapsed/round-trip metrics, report completeness and sampled maxima, and apply unknown/blocking rules. Host APFS retention is explicitly distinguished from the live-image bound. C7's live-detach exception is narrow and does not relax production stop-first cleanup.

12. **Round-1 #12 — Fixed at the root: ownership and evidence claims are now separated.** §2 lines 21, 41; §3 lines 94, 102–104; §6 lines 143, 145–157. Previous severity: nit; remaining design severity: none.

    Exact text: M3 is “codec/open/publication”, M4 is “全部 pending/lease/scheduling/deadline machine”, and “全部新卷/权限/协议/时序/恢复/真实接线、安全隔离/Stop/登录与模型评测仍 not_run”. The APFS/first-mount overclaims are removed. M5 expressly forbids VM-only success sealing before volume cleanup, covering current `cleanup.rs:202` and `recover.rs:108`.

    An implementer could still assign scheduling to the codec, infer completed native validation from a fake, or seal after only Tart deletion. Minimal fix: use the stated card ownership and complete resource barrier. The remaining density is a should-fix readability issue, detailed in finding 15.

**New defects and compatibility assessment.** I found no new unsafe ownership/destruction rule, guest-controlled host-follow/blocking permission, relaxed timing contract, dependency cycle, hidden renewal worker, or weakened owner/ADR 0017/0018 admission decision. Protected staging is on the same volume and outside the sole export, not another shared directory. Temporary command/provider subprocess handles belong to the existing caller/owner; the text does not authorize an observer, new host thread, daemon, manager or Drop cleanup. The guest supervisor is the previously required S11e boundary.

The revised request tables have finite memory caps and fail closed at exhaustion. The scheduling contract includes slow queries and cleanup rather than assuming `O_NONBLOCK` makes disk I/O bounded. Unestablished destructive-operation exclusion leads to quarantine. Pause/suspend semantics, service-observation coverage, storage peaks, filesystem timing and no-elevation behavior remain measured blockers, not stronger claims than the evidence. In particular, observing eight successful publications cannot certify a worst-case timing guarantee.

ADR 0017's inherited scope FD, live process/code identity, independent final native ACK, TCC, persistent external Stop and user-only resume remain mandatory (§3 lines 86, 92, 100, 106; §4 line 112; §6 line 143). ADR 0018 §5's DNS/LAN/metadata/IPv6/UDP/QUIC and endpoint-service-boundary evidence survives the removal of the superseded host-guard proposal. A `/32` rule or a DEFAULT mechanics result cannot supply those proofs. The Stop constants remain 100ms latch, 500ms release, 5s native and 4750ms driver; failure to measure the required behavior keeps real admission closed. No new must-fix is justified merely because a candidate may fail these explicit gates.

13. **Should-fix — Controller question: allow a fixed artifact upload to each throwaway clone.** §5 lines 116, 120, 122; M2 line 150; scope wording at §6 line 143.

    **Answer: yes.** For the DEFAULT-network, trusted, model-free mechanics experiment, copying the reviewed fixed-digest probe into each throwaway clone over the same restricted key-only SSH is acceptable and simpler. It needs no additional owner installation, GUI/TCC action, separately modified golden, or new service. This does not remove the existing experiment-scope/notice and host-operation review requirements, and it does not replace preparation/sealing required later by M7/M8. If a second test identity is not already available, the current permission-denied/unknown/defer rule still applies; upload is not authority to create accounts or elevate.

    **Trade-off/failure scenario:** pre-installation associates the program with a sealed image and avoids a per-attempt transfer. Upload removes the extra prepared-image lifecycle, but adds transfer/setup cost and a mutable guest pathname between verification and execution. A redirected destination, partial copy, different interpreter/dependency, or replacement after hashing could execute something other than the reviewed program. Both choices still trust the vetted guest OS and DEFAULT network environment; neither cures gateway exposure.

    **Conditions:**

    - Verify the reviewed artifact's pinned digest and size on the host, then verify the complete guest copy before executing it. Use a fixed ASCII destination in a fresh private directory of the limited test identity, outside the mailbox; reject links, special files, unexpected ownership/mode or replacement. Protect that directory/file from other test principals through execution. Pin required interpreter/dependency identities too; no package installation, external download, fetched script or guest-selected command.
    - Keep all line 120 SSH restrictions, pinned host key and exact clone/IP/boot binding for both transfer and launch. Authorize only this fixed upload and fixed verification/launch commands. `scp` is commonly SFTP-based: an existing forced-command-only key may reject that subsystem. Confirm compatibility rather than opening an unrestricted shell, general SFTP service or legacy shell-based fallback. If the existing restricted route cannot support the fixed transfer, mark this variant unavailable; do not add privileged configuration or another service to make the shortcut work.
    - Bound artifact bytes, output, connection/transfer/verification/execution and kill/reap within declared setup/attempt budgets. A lost result or digest mismatch ends the subcase; retries cannot reset aggregate ends. The probe retains its own finite workload deadline. Remove the copied artifact with the clone and independently prove clone/resource absence before the next subcase.
    - Treat SSH as code deployment/driver only. Host and guest independently generate synthetic workload patterns from the reviewed program/specification; do not send actual measured mailbox traffic, per-sample expected answers, leases, or authoritative results over SSH. A digest check over SSH validates the intended deployment within the trusted laboratory setup; it is not evidence that mailbox publication, mount or cleanup works. Keep DEFAULT-network/mechanics-only labels and M7/M9 block-all retests.

    **Minimal text changes:** replace line 116's sentence beginning “最小入口是事先…” with:

    > 最小入口是固定摘要的经审阅确定性探针程序；controller 可经同一受限 key-only SSH 将固定大小上限的程序复制到每个无个人数据/无登录态的一次性 DEFAULT clone 的固定私有位置，宿主核对源摘要、guest 核对完整副本摘要并排除链接/替换后，才以固定命令执行；随 clone 销毁，不修改 golden、不需 owner 安装，也不依赖 M7 supervisor/golden/sandbox。不能满足安全复制/执行条件即 unknown，不扩权或新增服务。

    Amend line 120's “只准固定经审阅命令” to include this one bounded fixed-destination upload plus verification/launch, retaining its prohibitions. Replace “预装探针本地亦有负载截止” with “探针本地亦有负载截止”. In line 122 change “两端预装程序独立生成” to “两端固定摘要程序按同一经审阅规格独立生成”; clarify that only the program artifact, not the measured file exchange/answers, uses the upload exception. Change M2's owner-action cell (line 150) to “复制探针无需 owner 安装；已有双主体可测，否则按§5 unknown/后移，不登录模型”. Scope line 143's no-general-file-exchange statement to preserve this single fixed-artifact laboratory exception. These are report recommendations, not edits made by this review.

14. **Should-fix — Separate the 100ms scheduler cadence from issuance of new inventory/lease requests.** §2 line 64; §3 lines 84, 94, 98, 102. This is an availability/implementation ambiguity, not a weakened safety bound.

    **Failure scenario:** an implementation creates a fresh inventory and lease every 100ms because the caller polls at that frequency. It exhausts the 2048-request tables in about 205 seconds, even within the five-minute login cap. The required response is safe revocation, but otherwise valid long attempts become impossible. The text currently specifies an upper interval for publication and a maximum request count without giving a compatible issuance plan.

    **Minimal fix:** state that 100ms is the maximum gap between servicing checks/pending progress, not a mandate to create a new sample each tick. M4 should demonstrate a bounded fresh-sample/grant schedule that fits both the 2048 caps over the supported preparation/task lifetime and measured grant-age/gap limits, with a long-login/maximum-task trace. Preserve fail-closed exhaustion and all existing deadlines; if no such schedule works, the transport fails its gate. No larger lease, thread or observer is implied.

15. **Should-fix — Split implementation instructions into readable units; cut repetition rather than constraints.** §2 lines 23–66; §3 lines 70–104; §5 lines 116–137; §6 lines 145–157.

    The file is 157 lines / 33,286 UTF-8 bytes; its longest physical line is line 104 at 597 characters. The problem is compound obligations per paragraph/table row, not the line count itself.

    **Failure scenario:** a reader finds the high-level decision but misses the last clause of a row, such as a lost-reply quarantine rule, aggregate deadline, consumption condition, or unknown observation. Different cards then implement different portions of one safety requirement.

    **Minimal fix:**

    - Split line 72 into ordered create/check/write/fsync/publish/verify/close steps, with separate source-protection and destination-race notes. Split line 74 into the corresponding read procedure. Retain same-FD dev/FSID/nlink checks, nonblocking open, finite directory/byte limits and the outer-process escape for blocking filesystem calls.
    - Expand line 25 into a short resource/identity table distinguishing R/A/I/S/fixed seed entries, M₀, V and mounted children. Keep the five destructive rules in one place and link to them from C5/M5; never shorten them to “recheck identity”.
    - Keep the seven-operation table to request fields, completion location and result fields. Move common typing, status/outcome rules, retained-terminal consumption and obsolete-lease rules into separate short paragraphs. Add one request→accepted→inventory-terminal→consume example without creating a second normative schema.
    - Split lines 102–104 into one caller-loop sequence and one deadline formula/origin list. Keep the single budget table. Remove the repeated fake-code shortcomings from line 104 once M4 carries their exact code references and required traces; retain the normative saved-end, no-restart and late-success rules here.
    - Turn C1–C10's long rows into short per-subcase blocks with three fields: finite workload, observation, pass/unknown criterion. Keep common attempt/output/teardown rules once at line 124. Put the SSH option inventory in a small laboratory-driver checklist, distinct from what SSH results can prove.
    - Cut repeated “round-1 recommendation adopted” narration and repeated no-model/no-sudo/no-admission prose where a link to one authoritative rule suffices. M1–M9 should name the deliverable, prerequisites and owner action, linking to the detailed contract instead of restating it. Do not cut the conditional blocking gates, Stop constants, irreversible expiry, exact ownership checks or native-evidence boundary.

Verification was local document/source inspection and report validation only. No network, Cargo/build/test commands, VM/volume/SSH operations, host changes or git commit were performed. All proposed native measurements remain **not_run**. Only this repo-root report file was written.
