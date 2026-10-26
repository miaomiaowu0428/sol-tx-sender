//! Portable Ed25519 sign / verify benchmark.
//!
//! Why: test whether a server with AVX-512 (Zen 4/5) actually signs Solana
//! transactions faster, and whether ed25519-dalek 3.0 / native codegen helps.
//!
//! Build (portable, runs anywhere):
//!   cargo build --release
//!
//! Build (native, run only on the target server):
//!   RUSTFLAGS="-C target-cpu=native" cargo build --release
//!
//! Build (AVX-512 tuned, run only on AVX-512 CPUs):
//!   RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512ifma,+avx2" cargo build --release
//!
//! Run:
//!   ./target/release/ed25519-bench
//!   ./target/release/ed25519-bench --check
//!   ./target/release/ed25519-bench --iters 5000 --rounds 9
//!   ./target/release/ed25519-bench --msg-len 1500   # 按生产 V1 消息大小测

use std::hint::black_box;
use std::time::Instant;

use ed25519_dalek as dalek2;
use ed25519_dalek_v3 as dalek3;
use solana_keypair::{Keypair, Signer as _};

#[derive(Clone, Copy)]
struct Cfg {
    iters: u32,
    rounds: usize,
    msg_len: usize,
    check_only: bool,
}

impl Default for Cfg {
    fn default() -> Self {
        Self { iters: 3000, rounds: 7, msg_len: 500, check_only: false }
    }
}

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs[xs.len() / 2]
}

/// 每个 case 独立测 3 组、每组 rounds 轮，取最好一组的中位数。
fn bench_best<F: FnMut()>(iters: u32, rounds: usize, mut f: F) -> f64 {
    for _ in 0..(iters / 10).max(10) {
        f();
    }
    let mut best = f64::MAX;
    for _ in 0..3 {
        let mut v = Vec::with_capacity(rounds);
        for _ in 0..rounds {
            let t0 = Instant::now();
            for _ in 0..iters {
                f();
            }
            v.push(t0.elapsed().as_secs_f64() * 1e6 / iters as f64);
        }
        best = best.min(median(v));
    }
    best
}

fn measure(iters: u32, f: &mut impl FnMut()) -> f64 {
    let t0 = Instant::now();
    for _ in 0..iters {
        f();
    }
    t0.elapsed().as_secs_f64() * 1e6 / iters as f64
}

/// 交替跑 A/B，抵消 CPU 频率漂移；返回 (A 中位, B 中位, 配对比值中位)。
fn paired_ab<A: FnMut(), B: FnMut()>(
    iters: u32,
    rounds: usize,
    a: &mut A,
    b: &mut B,
) -> (f64, f64, f64) {
    for _ in 0..(iters / 10).max(10) {
        a();
        b();
    }
    let mut va = Vec::new();
    let mut vb = Vec::new();
    let mut ratios = Vec::new();
    for r in 0..rounds {
        let (ta, tb) = if r % 2 == 0 {
            let ta = measure(iters, a);
            let tb = measure(iters, b);
            (ta, tb)
        } else {
            let tb = measure(iters, b);
            let ta = measure(iters, a);
            (ta, tb)
        };
        va.push(ta);
        vb.push(tb);
        ratios.push(tb / ta);
    }
    (median(va), median(vb), median(ratios))
}

fn cpu_model() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|v| v.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".into())
}

fn cpu_features() -> Vec<(&'static str, bool)> {
    #[cfg(target_arch = "x86_64")]
    {
        vec![
            ("avx2", std::arch::is_x86_feature_detected!("avx2")),
            ("avx512f", std::arch::is_x86_feature_detected!("avx512f")),
            ("avx512vl", std::arch::is_x86_feature_detected!("avx512vl")),
            ("avx512ifma", std::arch::is_x86_feature_detected!("avx512ifma")),
            ("bmi2", std::arch::is_x86_feature_detected!("bmi2")),
            ("adx", std::arch::is_x86_feature_detected!("adx")),
        ]
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        vec![]
    }
}

fn compiled_features() -> Vec<(&'static str, bool)> {
    vec![
        ("avx2", cfg!(target_feature = "avx2")),
        ("avx512f", cfg!(target_feature = "avx512f")),
        ("avx512vl", cfg!(target_feature = "avx512vl")),
        ("avx512ifma", cfg!(target_feature = "avx512ifma")),
    ]
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

fn print_header(cfg: Cfg) {
    println!("=== ed25519-bench ===");
    println!("host: {}", hostname());
    println!("cpu:  {}", cpu_model());
    let rt = cpu_features()
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(" ");
    println!("runtime cpu flags: {rt}");
    let bt = compiled_features()
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(" ");
    println!("build-time target features: {bt}");
    println!("message: {} B; sign iters: {}; rounds: {}", cfg.msg_len, cfg.iters, cfg.rounds);
    println!();
}

fn main() {
    let mut cfg = Cfg::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--iters" => {
                cfg.iters = args.next().and_then(|s| s.parse().ok()).unwrap_or(cfg.iters)
            }
            "--rounds" => {
                cfg.rounds = args.next().and_then(|s| s.parse().ok()).unwrap_or(cfg.rounds)
            }
            "--msg-len" => {
                cfg.msg_len = args.next().and_then(|s| s.parse().ok()).unwrap_or(cfg.msg_len)
            }
            "--check" => cfg.check_only = true,
            "-h" | "--help" => {
                println!("usage: ed25519-bench [--iters N] [--rounds N] [--msg-len BYTES] [--check]");
                return;
            }
            other => {
                eprintln!("unknown arg: {other}");
                std::process::exit(2);
            }
        }
    }

    print_header(cfg);
    if cfg.check_only {
        return;
    }

    let seed = [7u8; 32];
    let message = vec![0xABu8; cfg.msg_len];

    let sol_kp = Keypair::new_from_array(seed);
    let sk2 = dalek2::SigningKey::from_bytes(&seed);
    let vk2 = sk2.verifying_key();
    let sk3 = dalek3::SigningKey::from_bytes(&seed);
    let vk3 = sk3.verifying_key();
    assert_eq!(vk2.to_bytes(), vk3.to_bytes());
    let ring_kp = ring::signature::Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();

    // ---------- sign ----------
    println!("[sign] {} B message, us per signature (lower is better)", cfg.msg_len);
    let t_sol = bench_best(cfg.iters, cfg.rounds, || {
        black_box(sol_kp.sign_message(black_box(&message)));
    });
    let t2 = bench_best(cfg.iters, cfg.rounds, || {
        black_box(dalek2::Signer::sign(&sk2, black_box(&message)));
    });
    let t3 = bench_best(cfg.iters, cfg.rounds, || {
        black_box(dalek3::Signer::sign(&sk3, black_box(&message)));
    });
    let t_ring = bench_best(cfg.iters, cfg.rounds, || {
        black_box(ring_kp.sign(black_box(&message)));
    });
    println!("  {:<32} {:>8.2}", "solana Keypair (dalek 2.2)", t_sol);
    println!("  {:<32} {:>8.2}", "ed25519-dalek 2.2", t2);
    println!("  {:<32} {:>8.2}", "ed25519-dalek 3.0", t3);
    println!("  {:<32} {:>8.2}", "ring", t_ring);

    // Interleaved A/B: the reliable way to compare 2.2 vs 3.0 on a noisy box.
    let mut f2 = || {
        black_box(dalek2::Signer::sign(&sk2, black_box(&message)));
    };
    let mut f3 = || {
        black_box(dalek3::Signer::sign(&sk3, black_box(&message)));
    };
    let (_a2, _a3, ratio) = paired_ab(cfg.iters.max(1000), cfg.rounds, &mut f2, &mut f3);
    println!("  paired dalek3/dalek2 ratio (median) = {ratio:.4}");
    println!();

    // ---------- verify ----------
    let n = 64usize;
    let msgs: Vec<Vec<u8>> = (0..n).map(|i| vec![(i % 251) as u8; cfg.msg_len]).collect();
    let sigs2: Vec<_> = msgs.iter().map(|m| dalek2::Signer::sign(&sk2, m)).collect();
    let msg_refs: Vec<&[u8]> = msgs.iter().map(|m| m.as_slice()).collect();
    let vks2 = vec![vk2; n];

    let ver_iters = 30u32;
    let seq_us = bench_best(ver_iters, cfg.rounds, || {
        for i in 0..n {
            black_box(vk2.verify_strict(&msgs[i], &sigs2[i]).is_ok());
        }
    }) / n as f64;
    let batch_us = bench_best(ver_iters, cfg.rounds, || {
        black_box(ed25519_dalek::verify_batch(&msg_refs, &sigs2, &vks2).is_ok());
    }) / n as f64;

    let sigs3: Vec<_> = msgs.iter().map(|m| dalek3::Signer::sign(&sk3, m)).collect();
    let seq3_us = bench_best(ver_iters, cfg.rounds, || {
        for i in 0..n {
            black_box(vk3.verify_strict(&msgs[i], &sigs3[i]).is_ok());
        }
    }) / n as f64;

    println!("[verify] n={n}, {} B messages, us per signature", cfg.msg_len);
    println!("  {:<32} {:>8.2}", "dalek 2.2 sequential", seq_us);
    println!("  {:<32} {:>8.2}", "dalek 2.2 verify_batch", batch_us);
    println!("  {:<32} {:>8.2}", "dalek 3.0 sequential", seq3_us);
    println!("  batch speedup vs seq: x{:.2}", seq_us / batch_us);
    println!();

    // ---------- parallel sign ----------
    let hw = std::thread::available_parallelism().map(|v| v.get()).unwrap_or(1);
    println!("[parallel sign] available_parallelism={hw}");
    let per_thread = 2000u32;
    for threads in [1usize, 2, 4, 8] {
        if threads > hw * 2 {
            continue;
        }
        let mut best = f64::MAX;
        for _ in 0..5 {
            let t0 = Instant::now();
            std::thread::scope(|s| {
                for _ in 0..threads {
                    s.spawn(|| {
                        let kp = Keypair::new_from_array(seed);
                        for _ in 0..per_thread {
                            black_box(kp.sign_message(black_box(&message)));
                        }
                    });
                }
            });
            best = best.min(t0.elapsed().as_secs_f64() * 1e6 / (threads * per_thread as usize) as f64);
        }
        println!("  {threads:>2} thread(s): {best:>6.2} us/sig   ({:>8.0} sig/s)", 1e6 / best);
    }

    // sanity
    let sig = sol_kp.sign_message(&message);
    let dsig = dalek2::Signature::from_slice(&<[u8; 64]>::try_from(sig.as_ref()).unwrap()).unwrap();
    vk2.verify_strict(&message, &dsig).unwrap();
}