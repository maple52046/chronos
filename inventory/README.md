# Inventory

`inventory/` 存放**環境的營運狀態（operational state）**——目前最主要是 server 清單
（host / user / key path，語意對齊 ssh client config）。

這一層**不是 knowledge**：它描述「我有哪些機器、怎麼連」，是會變動、環境特定的狀態，
而不是「某個東西怎麼運作」的長期知識。因此獨立成一層，避免污染 `knowledge/`。

## 檔案

| 檔案 | 進版控？ | 用途 |
| --- | --- | --- |
| `servers.example.yaml` | ✅ 提交 | 結構範本，不含真實節點 |
| `servers.yaml` | ❌ gitignore | 真實清單，只存在本機 |
| `*.local.*` | ❌ gitignore | 其他本機專屬覆寫 |

## 使用

```bash
cp inventory/servers.example.yaml inventory/servers.yaml
# 編輯 servers.yaml 填入真實節點
```

## 規則

- **不放 secret。** 私鑰內容、密碼、token 一律不進此處；`key` 只放路徑。
- **真實狀態不進 git。** 只有 `servers.example.yaml` 的結構範本被提交。
- **引用方向。** `inventory/` 可引用 `knowledge/`（如 `knowledge/ops/wstunnel/`）；
  `knowledge/` 不反向引用此層。

## Schema

`servers[]` 每個項目：

| 欄位 | 必填 | 對應 ssh config | 說明 |
| --- | --- | --- | --- |
| `name` | ✅ | `Host` | 邏輯名稱 |
| `host` | ✅ | `HostName` | 連線目標 |
| `port` |  | `Port` | 預設 22 |
| `user` | ✅ | `User` | 登入帳號 |
| `key` |  | `IdentityFile` | 私鑰**路徑** |
| `host_key_alias` |  | `HostKeyAlias` | 走隧道時綁定真實 host key |
| `via` |  | — | 連線方式（如 `wstunnel`） |
| `tags` |  | — | 分類標籤 |
| `notes` |  | — | 備註 / 相關文件連結 |
