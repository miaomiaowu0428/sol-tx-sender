# ed25519-bench

在目标服务器上原生编译的 Ed25519 / Solana 签名基准。
**默认用 `-C target-cpu=native` 编译**：服务器有 AVX-512 就自动开 AVX-512，
不用手动记一堆 `-C target-feature`。只使用固定测试种子，不涉及真实私钥。

## 最快：直接从 GitHub 安装

服务器不用 clone 仓库，一条命令：

```console
cargo install --git https://github.com/miaomiaowu0428/sol-tx-sender.git \
  --branch ed25519-bench \
  --locked --force \
  --config 'build.rustflags=["-C","target-cpu=native"]'
```

然后：

```console
ed25519-bench --check
ed25519-bench --iters 5000 --rounds 9 | tee ~/avx512.txt

# 按生产 V1 消息大小测（重要：Ed25519 要对消息哈希两次，签名时间随消息变大）
ed25519-bench --msg-len 1500 --iters 5000 --rounds 9 | tee ~/avx512-1500.txt
```

> `ed25519-bench` 分支是一个**独立的单 crate 分支**（repo 根就是本 crate），
> 所以不需要 `--path` / 包名。
> `cargo install --git` 不会读取仓库里的 `.cargo/config.toml`，所以 `--config` 必须显式带上；
> 它的作用就是 `-C target-cpu=native`，在服务器上自动启用 AVX-512。

## 备用：拷贝源码包安装

## 推荐用法：cargo install（一条命令）

把源码包拷到服务器（或者直接拷整个目录）：

```console
scp sol-tx-sender/bench-ed25519/dist/bench-ed25519-src.tar.gz user@server:/tmp/
ssh user@server
tar xzf /tmp/bench-ed25519-src.tar.gz -C /tmp
cd /tmp/bench-ed25519

# 直接装；目录里的 .cargo/config.toml 已经设好 target-cpu=native
cargo install --path . --locked --force
```

如果 `cargo install --path .` 没读到目录里的配置（例如你不在这个目录执行），用这个等价命令：

```console
cargo install --path . --locked --force \
  --config 'build.rustflags=["-C","target-cpu=native"]'
```

装完直接运行：

```console
ed25519-bench --check          # 确认 build-time 里有 avx512f/avx512vl/avx512ifma
ed25519-bench --iters 5000 --rounds 9 | tee avx512-server.txt
```

`cargo install` 默认装到 `~/.cargo/bin/ed25519-bench`。

建议服务器 `rustc >= 1.89`（AVX-512 target features 从 1.89 起稳定；低于这个版本
`curve25519-dalek` 的 build.rs 会退回 AVX2/serial，`--check` 里 `avx512*=false` 是正常的）。

如果只想先确认环境，装完跑 `ed25519-bench --check` 就退出，不跑基准。

## 为什么这样就自动开 AVX-512

`-C target-cpu=native` 会让 rustc 按当前服务器 CPU 打开所有可用 target features；
在 Zen 4/5 上就包含 `avx512f`、`avx512vl`、`avx512ifma`。
`curve25519-dalek` 5 的 `build.rs` 检测到 `avx512ifma + avx512vl` 后，
会自动选择它的 `avx512` backend；curve25519-dalek 4 也会走运行时 AVX-512/AVX2 路径。

运行 `ed25519-bench --check`，如果看到：

```text
build-time target features: avx2=true avx512f=true avx512vl=true avx512ifma=true
```

就说明 AVX-512 已经编进去了。

> 不要用 `-C target-cpu=znver4` 在本机交叉编给别的服务器：build script 会按 znver4 生成，
> 在非 Zen4 构建机上直接 SIGILL。`native` 是“在目标机上编”，正好符合这个场景。

## 输出怎么读

```text
[sign] 500 B message, us per signature (lower is better)
  solana Keypair (dalek 2.2)          14.20   <- 生产路径
  ed25519-dalek 2.2                   13.86
  ed25519-dalek 3.0                   14.11
  ring                                20.11
  paired dalek3/dalek2 ratio (median) = 0.9878
```

- `[sign]`：单次签名延迟，越低越好；`solana Keypair` 等价于 `sol-tx-sender` 的签名耗时。
- `paired dalek3/dalek2 ratio`：同进程交错 A/B 的配对比值；
  `< 1` 表示 3.0 更快，`≈ 1` 表示没区别。
- `[verify]`：验证侧对照；`batch speedup` 是 batch vs 逐条 verify。
- `[parallel sign]`：多线程摊薄吞吐；签名不能被“批量”加速，只能并行。

## 预期

- 有 AVX-512 的服务器，**单次签名也不会出现 4–5x 提升**：
  Ed25519 签名只有一次固定基点标量乘，AVX-512 主要帮 batched verify。
- 如果 `solana Keypair` 在 native/AVX-512 构建下只快几个百分点，就说明对签名没意义。
- 如果 `paired dalek3/dalek2 ratio` 明显 `< 0.95`，才值得考虑升 3.0
  （还要先解决 `solana-keypair` 绑定 2.x 的问题）。

## 备用：静态二进制（服务器没有 Rust 时）

`dist/` 里还有两个已经编好的静态程序：

- `ed25519-bench-static`：通用 x86_64，任何机器能跑；
- `ed25519-bench-avx512-static`：`+avx512f,+avx512vl,+avx512ifma` 编译版，
  只能在支持 AVX-512 IFMA 的 CPU 上跑。

```console
chmod +x ed25519-bench-*
./ed25519-bench-static --check
./ed25519-bench-static --iters 5000 --rounds 9
./ed25519-bench-avx512-static --iters 5000 --rounds 9   # 确认 avx512ifma=true 再跑
```

## 手动 build（可选）

```console
cargo build --release
./target/release/ed25519-bench --iters 5000 --rounds 9
```

`.cargo/config.toml` 会自动带上 `target-cpu=native`。

## 依赖

- `solana-keypair 3.1.2`：与 `solana-sdk 4.0` 相同的签名实现；
- `ed25519-dalek 2.2`（`fast` 默认开）和 `3.0` 各一份，用于对比；
- `ring`：作为第三方实现对照。