# P2P.ORG Syncro Sender — 对接文档（整理）

来源：
- <https://docs.p2p.org/docs/syncro-sender-overview>
- <https://docs.p2p.org/docs/syncro-sender-quick-start>
- <https://docs.p2p.org/docs/syncro-sender-optimizing-performance>
- <https://docs.p2p.org/docs/syncro-sender-pricing-rate-limits>

## 定位

Syncro Sender 通过 **SWQoS（Stake-Weighted Quality of Service）** 把交易直接投递到
当前/即将出块的 validator leader；每次 `sendTransaction` 会**并行走多通道**，单次调用
已内含冗余，客户端一般不需要自己重试投递。

## 端点

### HTTP（端口 8080）

| 区域 | URL |
|---|---|
| Frankfurt | `http://fra.sender.syncro.p2p.org:8080` |
| Amsterdam | `http://ams3.sender.syncro.p2p.org:8080` |
| Washington, DC | `http://us.sender.syncro.p2p.org:8080` |

- 私有端点：`/`（或 `/rpc`），需要 API key。
- 公开端点：`https://sfls.l2.p2p.org/public`，无需 key，但交易必须自带 tip。

> 实测：上述区域端点的 `/public` 返回 **HTTP 404**，公开流量请走 `sfls.l2.p2p.org/public`。

### QUIK（端口 8090）

`http://{ams3,fra,us}.sender.syncro.p2p.org:8090`，仅授权客户可用（需提交公钥）。

## 鉴权与费率

| | 公开（tip 付费） | 私有（API key） |
|---|---|---|
| 鉴权 | 交易内 SystemProgram transfer tip | `Authorization: Bearer <API_KEY>`（优先）或 `X-Api-Key: <API_KEY>` |
| 最低 tip | **200,000 lamports** | **150,000 lamports** |
| 速率 | 1 RPS / IP | 50 RPS / client（可定制） |
| 支持方法 | **仅 `sendTransaction`** | 全部 Solana RPC 方法 |

## Tip 账户（9 个，公开/私有共用）

```
BPZrtYhdoAhiHWV5EgGLoV7bZFbMamBZurGDq4DmST8v
7D5pdbkV75Sr73M1YFNZwXMed6DenwkdfbJwVWrX6drQ
ELpn2NryEW4B3psG36eSjF45YcGMQpGGuu9J2AgAccbV
FnckAPC9PitnRpGZM2M4WLwb3w9odRLJ7EDRZDngjvd6
3ZnDTgvVfwzqwWoqAUmDkgVtXvXqjmeb5t9zxD5pMbmv
3SLDFcdCzMbcFNguZhzmV4zqEAUvcPoKY13akpE4Tq1p
48tT6LJqrsoFrLpzZSHkjGdGTWtsJ1PvjgWZjh8qF1RK
7GM9fpVMHHcrK4cgzfVdzJvjiy1bSyfwSYzhxvgbfVLg
CBd8GE3ffMJKf3iCCcNNBEifMxH1WpgtTzRnXPxxbjGE
```

tip 缺失/不足时返回 `-32602`：
- `Missing tip: no transfer to tip accounts found`
- `Insufficient tip: provided X lamports, required Y lamports`

## 请求示例

公开：

```bash
curl -X POST https://sfls.l2.p2p.org/public \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"sendTransaction",
       "params":["<BASE64_TX_WITH_TIP>",{"encoding":"base64"}]}'
```

私有：

```bash
curl -X POST http://fra.sender.syncro.p2p.org:8080 \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -d '{"jsonrpc":"2.0","id":1,"method":"sendTransaction",
       "params":["<BASE64_TX>",{"encoding":"base64"}]}'
```

成功返回：

```json
{ "jsonrpc": "2.0", "result": "<TRANSACTION_SIGNATURE>", "id": 1 }
```

## 错误码

| Code | Name | HTTP | 说明 |
|---|---|---|---|
| `-32700` | PARSE_ERROR | 400 | JSON 解析失败 |
| `-32600` | INVALID_REQUEST | 400/403 | 结构非法或鉴权失败 |
| `-32601` | METHOD_NOT_FOUND | 200 | 不支持的方法 |
| `-32602` | INVALID_PARAMS | 200/400 | 参数非法 / 缺 tip / tip 不足 |
| `-32603` | INTERNAL_ERROR | 500 | 服务端内部错误 |
| `-32000` | SERVER_ERROR | 500 | 一般服务端错误 |
| `-32005` | RATE_LIMIT | 429 | 超速率 |

429 响应头：`X-RateLimit-Limit` / `X-RateLimit-Remaining` / `Retry-After`（秒，≥1）。

## 最佳实践

- `skipPreflight: true`（本 crate 默认带）。
- base64 编码（本 crate 走 `serialize_transaction_wire_base64`，即 Solana wire 格式）。
- 复用 HTTP 连接（server keep-alive 30s）。
- 同时带 **tip** 与 **priority fee**（`ComputeBudgetInstruction::set_compute_unit_price`）。
- 读操作走独立 RPC；Syncro 只负责投递，确认另用 `getSignatureStatuses`。
- 429 按 `Retry-After` 退避；400 不要重试；500/网络错误指数退避。

## Bundle 支持：❌ 不支持

文档对公开端点的「Supported methods」明确为 **`sendTransaction` only**，对私有端点是
**全部 Solana RPC 方法** —— `sendBundle` 不是标准 Solana RPC 方法，**两端点都未提供**。

实测（三个区域 × `/`、`/rpc` 路径，`method: "sendBundle"`）均返回：

```json
{"jsonrpc":"2.0","result":null,
 "error":{"code":-32601,"message":"Only sendTransaction is supported"},"id":1}
```

因此 `sol-tx-sender` 的 `syncro` 模块**只实现单笔发送**，不实现 `SendBundle` /
`BundleSender`。若官方后续开放 bundle，按 `src/platform_clients/syncro.rs` 顶部注释的
三步补齐即可。

## 在 sol-tx-dispacher 中的接入

`sol-tx-dispacher` 打开 `syncro` feature 后：

```rust
TxDispacher::builder(oracle)
    // 带 key 走私有端点（150k lamports 最低 tip、50 RPS）
    .syncro(Syncro::init_with(std::env::var("SYNCRO_API_KEY").unwrap_or_default(), region))
    .build();
```

路由：`slot_leader.name == "P2P.org Turbo"`（见 `sol_slot_leader::LeaderInfo::is_p2p_turbo`）
时 `resolve_route` 返回 `SendRoute::P2pTurbo` —— 在 fallback 全平台广播的**基础上额外
追加** Syncro 通道，不改动其它平台的行为。V0/V1、tip / cost / tip-only 五条 dispatch
路径均已覆盖。
