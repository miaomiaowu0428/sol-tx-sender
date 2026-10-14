# LandX Transaction Sender — 对接文档（整理）

来源：官方文档（由使用方提供，含 Basic Rules / Region Endpoints / Tip Addresses / Health 章节）。

## Basic Rules

- Base URL 同时支持 **HTTP** 与 **UDP** 两种提交方式。
- **HTTP**：API key 必须放在 **path** 里；**不支持** query-string 或 header 鉴权。
- **UDP**：API key 必须放在 **packet body 的前 12 字节**。
- 每笔交易必须包含一笔打给指定 tip 地址的 **SystemProgram transfer**。
- 默认最低 tip：**0.001 SOL**（= 1,000,000 lamports）。
- JSON-RPC 只使用 **base64** 编码。
- 尽可能复用 HTTP **keep-alive** 连接。

## Region Endpoints

| Region | Base URL | Provider | 提交方式 |
| --- | --- | --- | --- |
| `Amsterdam` | `ams1.landx.dev` | TeraSwitch | HTTP / UDP |
| `Amsterdam2` | `ams2.landx.dev` | Cherry Servers | HTTP / UDP |
| `Frankfurt1` | `fra1.landx.dev` | TeraSwitch | HTTP / UDP |
| `Frankfurt2` | `fra2.landx.dev` | Allnodes | **仅 UDP** |
| `Frankfurt3` | `fra3.landx.dev` | Cherry Servers | **仅 UDP** |
| `New York` | `ny1.landx.dev` | TeraSwitch | HTTP / UDP |
| `Tokyo` | `tyo1.landx.dev` | TeraSwitch | HTTP / UDP |
| `London` | `lon1.landx.dev` | TeraSwitch | HTTP / UDP |
| `Singapore` | `sgp1.landx.dev` | TeraSwitch | HTTP / UDP |
| `Salt Lake City` | `slc1.landx.dev` | TeraSwitch | **仅 UDP** |
| `Dublin` | `dub1.landx.dev` | TeraSwitch | **仅 UDP** |

**Frankfurt2 / Frankfurt3 / Salt Lake City / Dublin 目前仅支持 UDP。**
其余端点支持全部三种提交方式：

- HTTP JSON-RPC
- HTTP binary transaction
- UDP

## Tip Addresses

交易 tip 打给以下任意一个 Solana 地址（提升优先级）：

```text
LandX1EMgYtrfi4jVJ7ryby4mDpZcef6sVH9b9U1cog
LandX2gHE7y6RejuvoLJK4hJiKkrAwcrJJ34u19Gmo1
LandX3uK1cSUF7hbefKo3oL1j4FhvhxitiMRKut99Z6
LandX4wXzvDyWfq1XXn4StRjwPjzqb7je2kqDGLoT9L
LandX5T4jfRASrcn76owxHM4q9bduNGj24ev4Ufc5fP
LandX6RsjjnfaxfYAFTZX1TW9Cgfe9HFStj9nRDd5SK
LandX7LsdzXDAbbyBjAq5FxLTnGQaU3LojGQMLZCvkt
LandX8Gv3UkhjmgKdSEg4F7yG7Z2tiqhNDYDtxazydN
LandX9fhNN5S7fuBgekKBYeYvK1Sy9hbGvDmHsBGYFh
LandXXxDXaSS8MjrqG9nri51htrfM6V4R3zXDVCeV8R
```

## Health

```bash
curl -s "http://<host>/ping"
```

成功响应：

```text
ok
```

实测（`http://ams1.landx.dev/ping`、`http://fra1.landx.dev/ping`）返回 `200 ok`。

---

# 本仓库实现（`src/platform_clients/landx.rs`）

## 环境变量

| 变量 | 说明 | 默认 |
| --- | --- | --- |
| `LANDX_API_KEY` | API key，`LandX::new()` 读取 | 空（发送时报错） |

`REGION` 决定选哪个区域 host。**本实现只用 HTTP**，忽略 UDP，
所以仅 UDP 的机房会回退到最近的 HTTP 可用机房：

| REGION | 实际 host | 说明 |
| --- | --- | --- |
| `Frankfurt2` | `fra1` | fra2 仅 UDP |
| `SaltLakeCity` | `ny1` | slc1 仅 UDP |
| `Dublin` | `lon1` | dub1 仅 UDP |

其余区域直接用对应机房；无 LandX 机房的区域就近回退
（LosAngeles/Pittsburgh→ny1，Limburg/Lithuania/Unknown→fra1）。

## 提交方式（实测确认 HTTP path 鉴权）

### HTTP JSON-RPC（唯一实现）

```http
POST http://<host>/<api_key>
Content-Type: application/json

{"jsonrpc":"2.0","id":1,"method":"sendTransaction",
 "params":["<base64 wire tx>",{"encoding":"base64","skipPreflight":true}]}
```

实测：`POST http://ams1.landx.dev/` → `404 not found`；
`POST http://ams1.landx.dev/DUMMYKEY` → `401 Missing or invalid authentication credentials.`
→ 证实 **key 在 path**。

文档里还提到 HTTP binary transaction 与 UDP，本仓库暂不实现：

- binary：形如 `Content-Type: application/octet-stream` + wire 裸字节，
  加的时候务必用 `serialize_transaction_wire`（wincode），**不能用 bincode**。
- UDP：文档只说「key 放在 packet body 前 12 字节」，未给端口与封装示例，暂缓。

## 行为约定

- `BuildTx::uses_tip_transfer()` 保持 `true`：文档要求每笔交易都要带 tip。
- `BuildTx::get_min_tip_amount()` = `1_000_000` lamports（0.001 SOL）。
- `tip_recvs()` 返回全部 10 个官方 tip 地址；BuildTx 随机取一个写入 tip transfer。
- 未实现 `sendBundle` / `BundleSender`：文档没有描述 bundle 协议。

## Keep-alive

`endpoint_keep_alive()` 会探测所有 HTTP 平台端点。
LandX 使用官方健康检查 `GET http://<host>/ping`（返回 `ok`）。
