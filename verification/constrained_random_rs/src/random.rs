// Match Python random.Random's integer seeding and getrandbits-based randrange.
// Keeping the stream identical preserves published seed reproductions.
const N: usize = 624;
const M: usize = 397;

pub struct PythonRandom {
    mt: [u32; N],
    index: usize,
}

impl PythonRandom {
    pub fn new(seed: i64) -> Self {
        let mut value = seed.unsigned_abs();
        let mut words = Vec::new();
        loop {
            words.push(value as u32);
            value >>= 32;
            if value == 0 {
                break;
            }
        }
        let mut rng = Self {
            mt: [0; N],
            index: N,
        };
        rng.init_genrand(19_650_218);
        let mut i = 1_usize;
        let mut j = 0_usize;
        for _ in 0..N.max(words.len()) {
            let prev = rng.mt[i - 1];
            rng.mt[i] = (rng.mt[i] ^ ((prev ^ (prev >> 30)).wrapping_mul(1_664_525)))
                .wrapping_add(words[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                rng.mt[0] = rng.mt[N - 1];
                i = 1;
            }
            if j >= words.len() {
                j = 0;
            }
        }
        for _ in (1..N).rev() {
            let prev = rng.mt[i - 1];
            rng.mt[i] = (rng.mt[i] ^ ((prev ^ (prev >> 30)).wrapping_mul(1_566_083_941)))
                .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                rng.mt[0] = rng.mt[N - 1];
                i = 1;
            }
        }
        rng.mt[0] = 0x8000_0000;
        rng
    }

    fn init_genrand(&mut self, seed: u32) {
        self.mt[0] = seed;
        for i in 1..N {
            let prev = self.mt[i - 1];
            self.mt[i] = 1_812_433_253_u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            for i in 0..N {
                let y = (self.mt[i] & 0x8000_0000) | (self.mt[(i + 1) % N] & 0x7fff_ffff);
                self.mt[i] =
                    self.mt[(i + M) % N] ^ (y >> 1) ^ if y & 1 != 0 { 0x9908_b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    pub fn range(&mut self, upper: usize) -> usize {
        assert!(upper > 0);
        let bits = usize::BITS - upper.leading_zeros();
        loop {
            let value = (self.next_u32() >> (32 - bits)) as usize;
            if value < upper {
                return value;
            }
        }
    }

    pub fn choice<T: Copy>(&mut self, values: &[T]) -> T {
        values[self.range(values.len())]
    }

    pub fn shuffle<T>(&mut self, values: &mut [T]) {
        for i in (1..values.len()).rev() {
            let j = self.range(i + 1);
            values.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PythonRandom;

    #[test]
    fn python_seed_and_randrange() {
        let mut rng = PythonRandom::new(1);
        assert_eq!(rng.next_u32(), 577_090_037);
        assert_eq!(rng.next_u32(), 2_444_712_010);
        assert_eq!(rng.range(8), 1);
    }
}
