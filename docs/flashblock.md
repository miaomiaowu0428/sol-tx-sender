# FlashBlock — 对接文档（整理）

本仓库有两个 FlashBlock 发送器：

| 发送器 | 代码 | 协议 / 端点 |
| --- | --- | --- |
| HTTP | `src/platform_clients/flash_block.rs` | REST `POST /api/v2/submit-batch`，`http://<region>.flashblock.trade` |
| QUIC | `src/platform_clients/flash_block_quic.rs` | FLQ1 v1，`<region>.flashblock.trade:6888`（UDP） |

来源（2026-10 同步）：FlashBlock《⚡ QUIC Submit》官方文档（FLQ1 v1，2026-07-28 版）。

## QUIC（FLQ1 v1）

| 项 | 值 |
| --- | --- |
| 传输 | QUIC over UDP |
| 端口 | **6888** |
| TLS | 1.3，**ALPN = `flash-tx/1`**（硬要求，缺了服务端不认） |
| 认证 | 现有 FlashBlock API key（与 HTTP 同一个） |
| payload | 已签名的 Solana **wire** 字节（不是 base64，不是 bincode） |
| 单笔上限 | **4096** 字节 |
| 流 | 每帧一个 **client-initiated unidirectional** stream，必须 FIN |

### 帧格式

```text
magic[4]       = "FLQ1"
version[1]     = 1
type[1]        = 0x01 AUTH | 0x02 SUBMIT
flags[1]       = 0            ← v1 必须 0，非 0 的帧被静默丢弃
payload_len[2] = u16 BE
payload        = payload_len 字节
```

- 第一个 client uni stream **必须**是 AUTH，payload = API key 的**原始 UTF-8 字节**。
- 服务端初始只给 1 条 uni stream 额度，AUTH 成功后才会放开更多额度。
- 每个 SUBMIT 用**新的** uni stream，一条流一笔交易（不支持批量、不支持 MEV 模式）。

### ⚠️ 没有任何应用层响应

服务端**永不**为 AUTH / SUBMIT 开响应流。`send.stopped()` 返回 `None` 只表示
「QUIC 确认了字节 + FIN，且对端没发 `STOP_SENDING`」，它**不**证明：

- AUTH 被接受（认证失败是直接关连接）
- 帧通过了版本 / 类型 / flags / 长度 / 限速校验（不合法就静默丢弃）
- 进了 dispatcher、到达 validator、上链成功

所以 `SendTx::send_tx` 返回 `Ok(())` 只代表「发出去且传输层确认」。
**真正的结果只能靠签名去查**（RPC / FlashBlock 交易错误接口）—— 上层 confirm
本来就在等签名结果，所以流程不用改，但别拿 `Ok` 当「已上链」。

### TLS 校验

官方示例用公网根证书，本实现默认**真校验**：

- 根证书来自 `webpki-roots`（编译期内嵌，不依赖宿主机证书文件）
- 必须用 **FQDN** 连接（TLS SNI + 证书校验都需要域名，不能用裸 IP）
- 调试期可设 `FLASHBLOCK_QUIC_INSECURE=1` 跳过校验（会打 warn）

### 超时 / 保活

| 项 | 取值 |
| --- | --- |
| connect | 5s |
| AUTH 的 FIN 确认 | 3s |
| SUBMIT 的 FIN 确认 | 3s |
| keep_alive_interval | 15s |
| max_idle_timeout | 60s |

断线（`TimedOut` / `Closed` / `ConnectionLost`）→ 读锁发现 `close_reason()`
非空后单飞重连 + 重新 AUTH。连接级失败（`open_uni` / `write` / `finish`）重试一次；
对端 `STOP_SENDING` 或超时不重试（前者是明确拒绝，后者状态未知）。

### 与 HTTP 通道的关系

共用同一个 API key、同一套权限/吊销状态、**同一个 TPS 限额**，QUIC **没有**独立配额。
两条通道同时开会互相抢 TPS，按需要只开一条（用 feature `flash_block` /
`flash_block_quic` 控制）。

## 端点

QUIC 用「你账号分配到的**启用了 QUIC** 的区域 FQDN」，官方要求先确认区域可用。
默认沿用 HTTP 通道的区域映射（去掉 scheme）：

| Region | host |
| --- | --- |
| `NewYork` / `Pittsburgh` | `ny.flashblock.trade` |
| `SaltLakeCity` / `LosAngeles` | `slc.flashblock.trade` |
| `Amsterdam` / `Limburg` / `Lithuania` | `ams.flashblock.trade` |
| `Frankfurt` / `Frankfurt2` / `Unknown` | `fra.flashblock.trade` |
| `Singapore` / `Tokyo` | `singapore.flashblock.trade` |
| `London` / `Dublin` | `london.flashblock.trade` |

覆盖方式：`FLASHBLOCK_QUIC_HOST=ny.flashblock.trade`（可带 scheme / 端口，会被规范化）。

## 注册方式（dispatcher）

```toml
# sol-tx-dispacher/Cargo.toml —— feature 默认已开
flash_block_quic = []
```

环境变量 `FLASHBLOCK_KEY` 存在时，`TxDispacher::from_config` 会在启动时连一条
**本机地区**的 QUIC 长连接并 AUTH；失败只打 warn 并跳过该通道。

启动日志：

```
[FlashBlockQuic] 已连接并发送 AUTH fra.flashblock.trade:6888（服务端无回执）
```

## Tip

- 单笔最低 tip：`100_000` lamports（与 HTTP 通道一致）。
- 10 个官方 tip 账户随机选取（`FLASH_BLOCK_TIP_ACCOUNTS`），与 HTTP 通道共用。
