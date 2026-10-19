# Astralane — 对接文档（整理）

本仓库有两个 Astralane 发送器：

| 发送器 | 代码 | 协议 / 端点 |
| --- | --- | --- |
| HTTP（Iris） | `src/platform_clients/astralane.rs` | JSON-RPC / 二进制，`*.gateway.astralane.io/iris` |
| QUIC | `src/platform_clients/astralane_quic/` | QUIC 单连接多路复用，`<IP>:7000` |

来源（2026-10 同步）：

- Endpoints and Configs：<https://astralane.gitbook.io/docs/low-latency/endpoints-and-configs>
- QUIC Transaction Submission：<https://astralane.gitbook.io/docs/low-latency/submit-transactions/quic-transaction-submission>
- Send Txn Fee Tiers：<https://astralane.gitbook.io/docs/low-latency/send-txn-fee-tiers>
- WebSocket（未接入）：<https://astralane.gitbook.io/docs/low-latency/submit-transactions/websocket-transaction-submission>

## HTTP（Iris）端点

官方目前提供 Global Edge + 11 个区域端点。`REGION` 选择**单个**区域端点：

| Region | 主机 | IPv4 | 备注 |
| --- | --- | --- | --- |
| `Frankfurt` | `fr.gateway.astralane.io` | `185.191.117.97` | 推荐 |
| `Frankfurt2` | `fr2.gateway.astralane.io` | `45.139.132.160` | |
| `LosAngeles` | `la.gateway.astralane.io` | `74.118.142.151` | 旧文档称 San Francisco；旧 `lax` 已废弃 |
| `Tokyo` | `jp.gateway.astralane.io` | `189.1.164.31` | |
| `NewYork` | `ny.gateway.astralane.io` | `64.130.45.19` | |
| `Amsterdam` | `ams.gateway.astralane.io` | `64.130.43.43` | 推荐 |
| `Amsterdam 2` | `ams2.gateway.astralane.io` | `84.32.186.73` | Cherry Servers（当前 `Region` 无对应枚举） |
| `Limburg` | `lim.gateway.astralane.io` | `162.19.222.232` | |
| `Singapore` | `sg.gateway.astralane.io` | `67.209.54.176` | |
| `Lithuania` | `lit.gateway.astralane.io` | `84.32.97.47` | |
| `London` | `lon.gateway.astralane.io` | `63.254.168.71` | |

- Global Edge：`https://edge.astralane.io/iris`（常量 `ASTRALANE_GLOBAL_ENDPOINT`）。
- 官方建议**同时**广播到 Global Edge 和最近区域端点。本客户端不会自动双发；
  需要时由调度层额外注册一个实例并覆盖 `endpoint`。
- 支持 HTTPS；官方称直接用 IPv4 更快，并且只有 IPv4 支持 QUIC。

`Region` 没有对应机房的回退：

| Region | 回退到 |
| --- | --- |
| `Dublin` | London |
| `SaltLakeCity` | Los Angeles |
| `Pittsburgh` | New York |
| `Unknown` | Frankfurt |

### HTTP 调用

- 单笔：`sendTransaction`（`skipPreflight: true`、`mevProtect: true`），
  `api-key` 放在请求头。
- Bundle：二进制 `IrisB sendBatch`，`POST /irisb?api-key=...&method=sendBatch`，
  body 为 `[u16 BE len][wire tx]...`。

官方声称可用 `sendTransaction` / `sendBundle` / `sendIdeal` / `getNonce` /
`sendPaladin` / `getHealth`；本仓库只实现了前两个，其余暂无对应 trait。

## QUIC 端点

QUIC 使用 `IP:7000` 直连；官方另列 `:9000` 为 MEV-protected 端口，本实现默认走
`:7000`。区域与 HTTP 端点是同一批机房：

| Region | IP:PORT |
| --- | --- |
| `Frankfurt` | `185.191.117.97:7000` |
| `Frankfurt2` | `45.139.132.160:7000` |
| `LosAngeles` | `74.118.142.151:7000` |
| `Tokyo` | `189.1.164.31:7000` |
| `NewYork` | `64.130.45.19:7000` |
| `Amsterdam` | `64.130.43.43:7000` |
| `Amsterdam 2` | `84.32.186.73:7000` |
| `Limburg` | `162.19.222.232:7000` |
| `Singapore` | `67.209.54.176:7000` |
| `Lithuania` | `84.32.97.47:7000` |
| `London` | `63.254.168.71:7000` |

QUIC 区服回退规则与 HTTP 相同（见 `astralane_quic/config.rs`）。

## Tip 地址

官方共 17 个（8 个基础 + 9 个 Recently Added），HTTP 与 QUIC 共用：

```text
astrazznxsGUhWShqgNtAdfrzP2G83DzcWVJDxwV9bF
astra4uejePWneqNaJKuFFA8oonqCE1sqF6b45kDMZm
astra9xWY93QyfG6yM8zwsKsRodscjQ2uU2HKNL5prk
astraRVUuTHjpwEVvNBeQEgwYx9w9CFyfxjYoobCZhL
astraEJ2fEj8Xmy6KLG7B3VfbKfsHXhHrNdCQx7iGJK
astraubkDw81n4LuutzSQ8uzHCv4BhPVhfvTcYv8SKC
astraZW5GLFefxNPAatceHhYjfA1ciq9gvfEg2S47xk
astrawVNP4xDBKT7rAdxrLYiTSTdqtUr63fSMduivXK
AstrA1ejL4UeXC2SBP4cpeEmtcFPZVLxx3XGKXyCW6to
AsTra79FET4aCKWspPqeSFvjJNyp96SvAnrmyAxqg5b7
AstrABAu8CBTyuPXpV4eSCJ5fePEPnxN8NqBaPKQ9fHR
AsTRADtvb6tTmrsqULQ9Wji9PigDMjhfEMza6zkynEvV
AsTRAEoyMofR3vUPpf9k68Gsfb6ymTZttEtsAbv8Bk4d
AStrAJv2RN2hKCHxwUMtqmSxgdcNZbihCwc1mCSnG83W
Astran35aiQUF57XZsmkWMtNCtXGLzs8upfiqXxth2bz
AStRAnpi6kFrKypragExgeRoJ1QnKH7pbSjLAKQVWUum
ASTRaoF93eYt73TYvwtsv6fMWHWbGmMUZfVZPo3CRU9C
```

实现里随机选一个作为 tip 收款方，`tip_recvs()` 返回全部地址。

## 最低 tip 与限速（按账号档位）

官方按**滚动 7 天 tip 量**分档；升级立即生效，降级有 7 天宽限期：

| 档位 | 最低 tip（SOL） | 7 天 tip 量门槛 | 单笔 TPS | Bundle TPS |
| --- | --- | --- | --- | --- |
| Free | 0.001 | N/A | 5 | 0 |
| VIP 1 | 0.0001 | 4 SOL | 20 | 5 |
| VIP 2 | 0.0001 | 5 SOL | 40 | 10 |
| VIP 3 | 0.00001 | 8 SOL | 80 | 20 |

本仓库取值：

- HTTP 单笔 `MIN_TIP_AMOUNT_TX = 10_000`（VIP 3 档）。
- HTTP bundle `MIN_TIP_AMOUNT_BUNDLE = 100_000`（VIP 1/2 档）。
- QUIC 单笔 `MIN_TIP_AMOUNT_TX = 100_000`（VIP 1/2 档）。

若账号实际档位更低，需自行调高，否则交易可能因低于最低 tip 被丢弃。QUIC 超额
限速是**静默丢弃**（连接不断开），只能通过落地率观测。

## 其他限制

- QUIC：单 key 最多 10 连接、单连接 64 并发流、流读取超时 750 ms、交易上限
  1232 字节、空闲 30 s（客户端 25 s keep-alive）。
- WebSocket（未接入）：`wss://<host>:5500/iris`，同样是 fire-and-forget，
  交易上限 1232 字节；单 IP 最多 2 连接。
