/// PCG-XSH-RR 64/32 shared by training and browser checkpoint inference.
pub struct Rng(pub u64);
impl Rng {
    fn next_u32(&mut self) -> u32 {
        let old = self.0;
        self.0 = old.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let x = (((old >> 18) ^ old) >> 27) as u32;
        x.rotate_right((old >> 59) as u32)
    }
    pub fn uniform(&mut self) -> f64 { self.next_u32() as f64 / 4294967296.0 }
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() { v.swap(i, (self.next_u32() as usize) % (i + 1)); }
    }
}
