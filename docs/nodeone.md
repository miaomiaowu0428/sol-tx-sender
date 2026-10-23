# Node1 — 对接文档（整理）

本仓库有两个 Node1 发送器：

| 发送器 | 代码 | 协议 / 端点 |
| --- | --- | --- |
| HTTP | `src/platform_clients/nodeone.rs` | JSON-RPC `sendTransaction`，`https://<region>.node1.me` |
| QUIC | `src/platform_clients/nodeone_quic.rs` | UUID 认证 + 双向 stream，`<region>.node1.me:16666` |

来源（2026-10 同步）：Node1 官方《QUIC API》文档。
（`NODEONE_KEY` 用注册后拿到的 **Account UUID**；HTTP 与 QUIC 共用同一个 key。）

## 端点

HTTP 与 QUIC 是同一批机房，只是协议/端口不同：

| Region | 主机 | QUIC |
| --- | --- | --- |
| `NewYork` | `ny.node1.me` | `ny.node1.me:16666` |
| `Frankfurt` | `fra.node1.me` | `fra.node1.me:16666` |
| `Frankfurt2` | `fra2.node1.me` | `fra2.node1.me:16666` |
| `Amsterdam` | `ams.node1.me` | `ams.node1.me:16666` |
| `London` | `lon.node1.me` | `lon.node1.me:16666` |
| `Dublin` | `dub.node1.me` | `dub.node1.me:16666` |
| `Tokyo` | `tk.node1.me` | `tk.node1.me:16666` |
| `Singapore` | `sgp.node1.me` | `sgp.node1.me:16666` |

没有机房的区域就近回退（两个发送器一致）：

| Region | 回退到 |
| --- | --- |
| `Pittsburgh` / `LosAngeles` / `SaltLakeCity` | New York |
| `Limburg` / `Lithuania` | Amsterdam |
| `Unknown` | Frankfurt |

TLS 的 `server_name` 必须与目标域名一致（`nodeone_quic::NodeOneQuic::server_name_of`
负责从 `host:port` 剥端口）。

## QUIC 协议要点

```text
connect ──► open_bi() ──► write(16 字节 UUID) ──► finish() ──► 读 1 字节（0 = 成功）
之后每笔交易：
open_bi() ──► write(wire 交易字节) ──► finish() ──► 读 [2B status][4B msg_len][msg]
```

- 认证内容 = UUID 解析后的 **16 字节二进制**，不是 36 字符字符串。
  5 秒内没发完，服务端直接关连接。
- **每笔交易一个新双向 stream**，一条 stream 只发一笔。
- 请求体**没有长度前缀**；服务端靠 stream EOF（`finish()`）判断一笔结束。
  超过 5 秒不 `finish()` → 服务端回 `408`，**不关连接**（这笔丢了）。
- 响应帧：`status: u16 BE` + `msg_len: u32 BE` + `msg`（UTF-8）。
  `status == 200` 时 `msg` 是 JSON-RPC 结果，否则是错误信息。
- **认证成功后复用同一条连接**，不要每笔都 connect + auth。

### 序列化：为什么不用 `bincode::serialize`

官方示例写的是 `bincode::serialize(&versioned_tx)`。在本仓库的 SDK 版本下：

| 交易版本 | `bincode::serialize` | 结论 |
| --- | --- | --- |
| Legacy / V0 | 与 wire 字节**完全相同** | 等价，可用 |
| V1 | **不同**（长度前缀写成 u64） | 不是合法 wire，会被拒 |

所以 `NodeOneQuic` 统一用 `serialize_transaction_wire`（wincode）：
对 Legacy / V0 与文档要求等价，且额外兼容 V1。单测见 `tests/tx_wire_format.rs`。

## 客户端超时与保活

| 项 | 取值 | 说明 |
| --- | --- | --- |
| `CONNECT_TIMEOUT` | 5s | 服务端宕机 / 路径黑洞时不能无限等 |
| `AUTH_TIMEOUT` | 5s | 与服务端的 5 秒认证窗口对齐 |
| `SEND_TIMEOUT` | 5s | 含 open_bi / write / finish / 读响应 |
| `keep_alive_interval` | 15s | 防 NAT / LB 回收空闲连接 |
| `max_idle_timeout` | 60s | **连接级**，不是 stream 级 |

- 环境变量 `NODEONE_QUIC_ALPN`：文档没约定 ALPN，万一服务端强制要求，用它兜底
  （不用改代码重新发版）。
- 重连判断：`NodeOneQuic::close_reason()` + 发送时的 `open_bi/send/recv` 错误。

## 注册方式（dispatcher）

```toml
# sol-tx-dispacher/Cargo.toml —— feature 默认已开
nodeone_quic = []
```

只要环境里有 `NODEONE_KEY`，`TxDispacher::from_config` 就会在启动时连一条
**本机地区**的 QUIC 长连接并认证一次；认证失败只打 warn 并跳过该通道，
不影响其它平台启动。

与 `astralane_quic` 一样是**单地区**通道（连接是有状态的，不能像 HTTP 那样
用 `RegionPool` 按 leader 懒切换）。

## Tip

- 单笔最低 tip：`100_000` lamports（与 HTTP 通道一致，见
  `NodeOneQuic::MIN_TIP_AMOUNT_TX`）。
- 24 个官方 tip 账户随机选取（`NODEONE_TIP_ACCOUNTS`），与 HTTP 通道共用。
