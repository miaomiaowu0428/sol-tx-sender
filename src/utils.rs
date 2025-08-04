pub trait SolToLamport {
    fn to_lamport(self) -> u64;
}

impl SolToLamport for f64 {
    fn to_lamport(self) -> u64 {
        (self * 1_000_000_000.0) as u64
    }
}

impl SolToLamport for f32 {
    fn to_lamport(self) -> u64 {
        (self * 1_000_000_000.0) as u64
    }
}

impl SolToLamport for u64 {
    fn to_lamport(self) -> u64 {
        self * 1_000_000_000
    }
}

impl SolToLamport for u32 {
    fn to_lamport(self) -> u64 {
        (self as u64) * 1_000_000_000
    }
}

impl SolToLamport for u16 {
    fn to_lamport(self) -> u64 {
        (self as u64) * 1_000_000_000
    }
}

impl SolToLamport for u8 {
    fn to_lamport(self) -> u64 {
        (self as u64) * 1_000_000_000
    }
}

impl SolToLamport for i64 {
    fn to_lamport(self) -> u64 {
        (self as u64).saturating_mul(1_000_000_000)
    }
}

impl SolToLamport for i32 {
    fn to_lamport(self) -> u64 {
        (self as u64).saturating_mul(1_000_000_000)
    }
}
