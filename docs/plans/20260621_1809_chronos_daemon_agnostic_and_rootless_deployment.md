# Chronos: Daemon-Agnostic Status & Rootless Pluggable Output

- Consolidated: 2026-06-21 18:09
- Status: 已實作（兩來源計劃皆已落地並出貨容器映像）

## 1. Purpose

把兩份已定案/已實作的 AI 計劃整併為單一長期參考文件，記錄兩個彼此呼應的主題：
讓 Chronos **不綁定特定 NTP daemon**、且**部署可免 root／映像精簡**。涵蓋
`chronos-server` 的同步狀態來源，以及 `chronos-gateway` 的可插拔輸出後端與特權
模型。

## 2. Source Scope

整併自 `docs/plans/manuscripts/` 下兩份草稿（不含 `README.md`）：

- `20260620-chronyc-image-dependency.md` —— `chronos-server` 對 `chronyc` 的
  打包依賴與同步狀態 provider 收斂（已實作）。
- `20260621-gateway-pluggable-output-backends.md` —— `chronos-gateway` 可插拔
  輸出後端與免 root（已實作）。

共 2 份來源檔。

## 3. Consolidated Background

兩個問題都源自「實作/打包把單一 daemon（chrony）的限制升級成整個產品的限制」：

- **Server 端**：佈署時需臨時 build 衍生 image（官方 image + `apt-get install
  chrony`）才能讓 server 回報 `sync: synchronized`。根因是 chrony provider 直接
  shell 呼叫 `chronyc tracking`，但發佈 runtime 沒裝 chronyc → provider 回
  `unknown` → `/time` 的 `sync:"unknown"` → gateway 依規則拒收所有樣本。衍生
  image 只是補洞的繞法。
- **Gateway 端**：在 Ubuntu（預設 `systemd-timesyncd`、無 chrony、sudo 需密碼）
  上，照文件只能走「裝 chrony + gateway 以 root 跑」。這違反作者 v1 設計意圖：
  gateway 本就應能對應不同 NTP daemon，且其運作不應需要 root。根因是輸出後端
  被硬編成 chrony、config 強制 chrony，且 root 需求其實只來自 chrony SOCK 的
  IPC 權限，卻被升級成整個 gateway 的屬性；文件又把此缺口寫成「刻意設計／v2」。

兩者共同收斂方向：以 port/實作分離與設定選擇，讓元件與授時 daemon 解耦，並讓
預設部署精簡、免 root。

## 4. Confirmed Decisions

- **Server 預設 provider 改為 `system`**：以 `adjtimex(2)` 讀核心 NTP 狀態
  （`STA_UNSYNC`/`TIME_ERROR`），daemon 無關、無外部執行檔、不一定需 host
  network。
- **保留 `chrony` provider，但改原生實作**：以 Rust 直接走 chrony command 協定
  （UDP，`REQ_TRACKING`）解析 `leap_status`/`stratum`/`last_offset`，**不再需要
  容器內有 `chronyc`**；位址由 `time_status.chrony_address`（預設
  `127.0.0.1:323`）設定。移除 `ChronycStatusProvider` 與 `chronyc_path`。
  - 理由：`chrony-candm` crate 為 GPL-2.0，與本專案 MIT 不相容，故自行實作。
- **映像精簡化**：runtime 改 `gcr.io/distroless/static-debian13` + `nonroot`，
  以 `x86_64-unknown-linux-musl` 全靜態編譯；TLS 後端由 aws-lc-rs 改為 ring；
  healthcheck 改為 binary 子指令（純 `TcpStream` 打 `/healthz`），compose 不再
  依賴 curl。
- **Gateway 輸出後端可由設定選擇**：以 tagged-union `output` 取代必填 `chrony`，
  組裝根用 factory `match` 出實作（取代硬編）。
- **新增 `ntp_shm` 後端且可免 root**：實作落在**解凍後的 `chronos-ntp`**
  （定案；非新增 `chronos-ntpshm`，解決原 Open Question 1）。
- **特權 per-backend**：gateway 預設非 root；root 變成 `chrony_sock` 後端專屬、
  opt-in。
- **向後相容**：保留 `chrony:` 區段作為 `output: { type: chrony_sock }` 別名一個
  版本，偵測到時記 deprecation warning。
- **`time_t` 使用固定寬度 `i64`**：SHM 結構不依賴 deprecated 的 `libc::time_t`
  別名（在 musl target 會觸發 deprecation 警告），語意等價且 layout 不變。

## 5. Architecture and Design Principles

- **Clean Architecture / 依賴內向**：`chronos-core` 擁有跨界資料與 ports
  （`OutputBackend`、`TimeStatusProvider`），外層 crate 實作之，於組裝根接線。
  core 不得引入 I/O 或具體 driver。
- **Ports 與多實作**：
  - `OutputBackend`：`chronos-chrony`（SOCK refclock）、`chronos-ntp`（SHM
    refclock）；未來 `builtin_ntp_server`、`direct_clock_setter` 沿用同一 port。
  - `TimeStatusProvider`：`system`（adjtimex）、`chrony`（原生 command 協定）。
- **元件契約邊界**：gateway 契約很窄（取樣 → 過濾/估算 → 交給設定的 daemon），
  不得默默繼承 `chronos-server` 的決定/限制。
- **core 僅輕量內省**：為 status 顯示在 `OutputBackend` 加 I/O-free 的
  `target_description()`，不得把 I/O 帶進 core。
- **誠實文件原則**：文件須區分「已實作」與「規劃中」，per-backend 的提權需求要
  寫在後端層級，不可寫成整個元件的固有規則。

## 6. Functional Scope

- Server `/time` 在無 chrony 環境下也能回報正確 `sync` 狀態（預設 `system`
  provider）。
- Gateway 可在至少兩種輸出後端間以設定切換：`chrony_sock`（SOCK）、`ntp_shm`
  （ntpd/ntpsec SHM，`127.127.28.<unit>`）。
- Gateway 在 `ntp_shm` 後端下可以**非 root** 運作並校正時鐘。
- 既有 `chrony:` 設定不改即可解析運作（經文件化別名）。
- 出貨 systemd unit／容器範例：可免 root 的後端**預設非 root**；root 為後端專屬
  opt-in 變體。
- 容器 healthcheck 不依賴 curl/chronyc。

## 7. Constraints and Rules

- **R1 — daemon-agnostic**：輸出後端由設定決定，非重編譯/寫死；保留擴充路徑且
  不需重構 workspace。
- **R2 — gateway 不得一律需要 root**：若某後端在某環境需提權，需求須侷限於該
  後端並明確標註；至少一個後端可非 root 運作。
- **R3 — gateway ≠ server**：server 的限制不得被 gateway 默默繼承。
- **R4 — 誠實文件**：不得把現行限制（chrony-only、需 root）寫成刻意設計。
- **R5 — 向後相容**：既有 chrony 部署須續用；提供遷移路徑（舊區段保留一版作
  別名）。
- **授權限制**：不得引入與 MIT 不相容的依賴（如 GPL-2.0 的 `chrony-candm`）。
- **unsafe 規範**：每個 `unsafe` 區塊須附 `// SAFETY:`（比照
  `chronos-server` 既有 adjtimex 寫法）。

## 8. Data Model and Format Notes

- **chrony SOCK `sock_sample`**：native-endian、40-byte 結構（gateway 與 chronyd
  同主機）。
- **ntpd/ntpsec SHM `shmTime`**：SysV 共享記憶體，key = `0x4E545030 + unit`
  （"NTP0"）。64-bit Linux ABI 下為 96-byte 結構；seconds 欄位用固定寬度 `i64`
  對應 C `time_t`。寫入採標準交握：`mode=1; valid=0; count++; 寫欄位; count++;
  valid=1`，配 `write_volatile`。對應關係：`receiveTimeStamp =
  local_receive_unix_nanos`、`clockTimeStamp = local_receive +
  estimated_offset_nanos`。
- **SHM 權限**：段以 `perm`（預設 `0666`）建立；unit ≥ 2 慣例為可公開存取，
  讓非 root 程序可寫。
- **chrony command 協定**：UDP `REQ_TRACKING`，解析 `leap_status`/`stratum`/
  `last_offset`。
- **byte-layout 測試**：以 `size_of`/`offset_of!` 斷言 SHM 結構（比照 chrony 的
  layout 測試）。

## 9. CLI / API / Config Notes

- **Server**：`time_status.provider` 預設 `system`；`chrony` 為進階選項，位址用
  `time_status.chrony_address`（預設 `127.0.0.1:323`）。移除 `chronyc_path`。
- **Gateway `output`（tagged union, `type` 為判別子）**：
  - `chrony_sock`：`sock_path`、`refid`。
  - `ntp_shm`：`unit`（預設 2）、`perm`（預設 `"0666"`）、`precision`（預設 -1）。
  - 舊 `chrony:` 區段 = `output: { type: chrony_sock, ... }` 別名（deprecated）。
  - `resolve_output()` 解析；`output` 與 `chrony` 並存或皆缺則於 `validate()`
    報錯。
- **Status 端點**：後端無關的 `output { kind, target, last_write }`（取代原
  `chrony { sock_path, last_write }`）。
- **Healthcheck 子指令**：`chronos-server|chronos-gateway healthcheck`。
- **Packaging**：預設 `examples/config/gateway.yaml` 用 `ntp_shm`；root 變體為
  `gateway.chrony.yaml`、`docker-compose.gateway.chrony.yml`、
  `chronos-gateway-chrony.service`（compose 以 `ipc: host` 共用 SysV SHM）。

## 10. Implementation Plan

兩主題皆已完成；以下為落地對應檔案，供後續維護參考。

- **Server status provider（已實作）**：
  - `crates/chronos-core/src/status.rs`：`TimeProvider` 新增 `system` 變體。
  - `crates/chronos-server/src/status_provider.rs`：`SystemClockStatusProvider`
    （adjtimex）、`ChronyStatusProvider`（原生 command 協定）；移除
    `ChronycStatusProvider`。
  - `crates/chronos-server/src/{main.rs,config.rs}`：`build_provider`/
    `default_provider` 接線與預設。
  - `Dockerfile`：musl 靜態 + ring + distroless static + nonroot；移除 curl
    依賴。
- **Gateway 可插拔輸出（已實作）**：
  - `crates/chronos-gateway/src/config.rs`：`OutputConfig` tagged union +
    `chrony:` 別名 + `resolve_output()`。
  - `crates/chronos-gateway/src/main.rs`：`build_output()` factory（取代硬編）。
  - `crates/chronos-ntp/*`：解凍，實作 `ShmRefclockBackend`（`shm_refclock.rs`
    定義 `#[repr(C)] ShmTime`/`publish_sample`；`writer.rs` 以
    `shmget(IPC_CREAT|perm)`＋`shmat`）。
  - `crates/chronos-core/src/sample.rs`：`OutputBackend::target_description()`。
  - `crates/chronos-gateway/src/status_api.rs`：`OutputInfo` 一般化。
  - packaging/examples：預設非 root + opt-in chrony 變體。
  - docs：`architecture.md`、`chrony-integration.md`、`deployment-gateway.md`
    修正，新增 `ntp-shm-integration.md`。
- **驗證**：`cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D
  warnings`、`cargo test`；容器映像已建置並推送。

## 11. Non-goals

- `systemd-timesyncd` **不列為** gateway 輸出目標：它是 SNTP client，無
  SOCK/SHM refclock 輸入。timesyncd 主機應作為 gateway 主機的**下游 NTP
  client**（文件須明寫以免混淆）。
- 不實作 builtin NTP server 或 direct clock setter（保留為未來後端）。
- 衍生 image `chronos-with-chronyc:*` 已被官方 image 內建 provider 取代，廢棄。

## 12. Open Questions

來源計劃多數已於實作定案，剩餘待決：

- **SHM 安全取捨**：world-writable（`0666`）SHM 的安全性；是否提供 `group` 旋鈕
  以 `0660` 收斂為同群組可寫。
- **`chrony_sock` 免 root**：是否向 chrony 上游提需求（可設定的 socket
  mode/group），以便 `chrony_sock` 亦可去 root。
- **`chrony:` 別名移除時程**：保留幾個版本後移除。

（已解決：crate 佈局 → 定案解凍 `chronos-ntp`，不新增 `chronos-ntpshm`。）

## 13. Future Work

- 新增 `builtin_ntp_server`、`direct_clock_setter` 輸出後端（沿用 `OutputBackend`
  port；後者需 root / `CAP_SYS_TIME`，屬後端專屬提權）。
- 完成實驗室驗收：`ntp_shm` 以非 root 跑、`ntpq -p` 顯示 refclock reachable；
  `chrony_sock` 迴歸；舊 `chrony:` 設定仍可運作。
- 視需要在 server 端補更完整的 chrony 資訊或其他 daemon 的原生 status provider。
- 若 chrony 上游提供可設定 socket 權限，更新特權矩陣使 `chrony_sock` 去 root。
