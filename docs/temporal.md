# Temporal (Nozomi) — 对接文档（整理）

本仓库只有**一个** Temporal 发送器：`src/platform_clients/temporal.rs`（HTTP）。
**没有** QUIC/H3 实现，也不建议加 —— 理由见下。

来源（2026-10 同步）：

- Batch Send：<https://use.temporal.xyz/nozomi/batch-send>
- QUIC Client：<https://use.temporal.xyz/nozomi/quic>
- 官方 QUIC 客户端：<https://github.com/temporalxyz/nozomi-quic-client>

## 为什么不实现 QUIC

官方文档原话：

> QUIC is **not** faster than the HTTP endpoints when you keep a warm connection
> open. Over a persistent, reused TCP connection, **Batch Send** over a direct
> `http://` endpoint is the lowest-latency path.

而且：

> The transport you choose does **not** change your priority in Nozomi's queue:
> priority is driven by your **tip**, not by QUIC vs HTTP.

QUIC（HTTP/3 over QUIC）只在「**无法保持长连接**、连接频繁重建」时有优势
（QUIC 的握手 + session resumption 比反复做 TCP+TLS 便宜）。

我们的情况正好相反：进程长活、`HTTP_CLIENT` 有连接池 + TCP keep-alive、
`TxDispacher::spawn_keep_alive` 还会定时探测保持热连接。所以：

- 上 H3 不会更快，**只是多一层复杂度**；
- 还要引入 `h3` + `h3-quinn` 等新依赖（这些**不在** `Cargo.lock`，也不在离线
  registry 缓存里），给最热的发送路径增加不确定性。

结论：**不实现 QUIC，改为把 HTTP 侧的 Batch Send 用起来**（这才是文档说的最快路径）。

## Batch Send 协议

| 项 | 值 |
| --- | --- |
| Method | `POST` |
| Path | `/api/sendBatch?c=<API_KEY>` |
| Content-Type | `application/octet-stream` |
| Body | 二进制批次（见下） |
| 成功 | **200 + 空 body** |
| 失败 | 纯文本 `400` / `401` / `429` / `500` |

### Wire format

```text
[len_hi][len_lo][tx_bytes...][len_hi][len_lo][tx_bytes...]...
```

长度前缀 = 后续交易字节数的 **u16 大端**。没有 JSON、没有分隔符。

### 限制

| 约束 | 值 |
| --- | --- |
| 每批最多 | 16 笔 |
| 单笔最小 | 66 字节 |
| 单笔最大 | 1232（legacy/v0） / **4096（v1）** |
| body 最大 | 65,568 字节 |

⚠️ 注意：官方 `nozomi-quic-client` 仓库里的常量是旧值（body 19,744 / 单笔 1232），
**以文档为准**。本实现按版本判定单笔上限（v1 判别字节 `0x81` 在第 0 位）。

### 两个必须知道的语义

1. **不返回交易签名** —— 签名本来就该客户端自己算（`tx.sig()`）。
2. **流式处理、可能部分成功且不回滚** —— 第 N 笔失败时，前 N-1 笔可能已被受理。
   所以签名要提前记好，事后用链上结果对账。

## 本仓库实现

`Temporal::send_tx` 的策略：

```text
TEMPORAL_JSON_RPC=1 ?
  ├─ 是 → 直接走老 JSON-RPC
  └─ 否 → 先走 Batch Send
            ├─ 200        → 完成（最快路径）
            └─ 非 2xx      → warn 一条，回落一次 JSON-RPC
```

回落设计的目的：万一某个区域没开 `/api/sendBatch`（404/415）或网关不认
`octet-stream`，交易**仍然发得出去**，不会因为换路径直接丢单。同一签名重复提交
在链上只会 `AlreadyProcessed`，不会重复成交。

日志：

```
[temporal/batch] 已受理 sig=<...>              ← 走的是 Batch Send
[temporal/batch] 失败(HTTP 404: ...)，回落 JSON-RPC   ← 说明该区域 Batch Send 不可用
```

看到第二种日志且频繁出现，就把该区域退回 JSON-RPC（或直接设 `TEMPORAL_JSON_RPC=1`）
并找 Temporal 确认。

## 端点

直连（非 Cloudflare）端点，`http://` 比 `https://` 更快：

| Region | Host |
| --- | --- |
| `Frankfurt` / `Frankfurt2` / `Unknown` | `fra2.nozomi.temporal.xyz` |
| `Amsterdam` / `London` / `Dublin` / `Limburg` / `Lithuania` | `ams1.nozomi.temporal.xyz` |
| `NewYork` / `LosAngeles` / `Pittsburgh` / `SaltLakeCity` | `ewr1.nozomi.temporal.xyz` |
| `Tokyo` | `tyo1.nozomi.temporal.xyz` |
| `Singapore` | `sgp1.nozomi.temporal.xyz` |

官方还有 `pit1`（Pittsburgh）、`ash1`（Ashburn）、`lax1`（Los Angeles）、
`lon1`（London）、`edge`（geo-DNS 自动路由）未接入本仓库。

## 环境变量

| 变量 | 必需 | 作用 |
| --- | --- | --- |
| `TEMPORAL_KEY` | 是 | `?c=` 的 API key |
| `TEMPORAL_JSON_RPC` | 否 | 设 `1` 强制走老 JSON-RPC 路径（跳过 Batch Send） |

## Tip

- 单笔最低 tip：`1_000_000` lamports（0.001 SOL）。
- 17 个官方 tip 账户随机选取（`TEMPORAL_TIP_ACCOUNTS`）。
- 注意：**优先级由 tip 决定，与传输方式无关**。
